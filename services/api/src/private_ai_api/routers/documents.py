from __future__ import annotations

import asyncio
import hashlib
import json
import shutil
from datetime import UTC, datetime
from pathlib import Path
from typing import Annotated, Any
from uuid import uuid4

from fastapi import (
    APIRouter,
    BackgroundTasks,
    Depends,
    File,
    Form,
    HTTPException,
    Query,
    Response,
    UploadFile,
    status,
)

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.services.app_preferences import read_app_preferences

router = APIRouter(prefix="/documents", tags=["documents"])

TEXT_EXTENSIONS = {".txt", ".md", ".markdown", ".csv", ".json", ".yaml", ".yml"}


def _safe_filename(value: str) -> str:
    name = Path(value).name.strip().replace("\x00", "")
    return name or "document"


def _require_workspace(services: AppServices, workspace_id: str) -> None:
    workspace = services.database.fetch_one(
        "SELECT id FROM workspaces WHERE id = ?",
        (workspace_id,),
    )
    if not workspace:
        raise HTTPException(status_code=404, detail="Workspace not found")


async def _require_workspace_async(services: AppServices, workspace_id: str) -> None:
    """The same check, off the event loop. SQLite blocks whichever thread calls it."""
    await asyncio.to_thread(_require_workspace, services, workspace_id)


def _with_ingestion(
    services: AppServices,
    documents: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    if not documents:
        return []
    document_ids = [str(document["id"]) for document in documents]
    placeholders = ",".join("?" for _ in document_ids)
    jobs = services.database.fetch_all(
        f"SELECT * FROM jobs WHERE document_id IN ({placeholders}) "  # noqa: S608
        "ORDER BY updated_at DESC",
        tuple(document_ids),
    )
    latest: dict[str, dict[str, Any]] = {}
    for job in jobs:
        document_id = str(job.get("document_id") or "")
        if document_id and document_id not in latest:
            latest[document_id] = job

    decorated: list[dict[str, Any]] = []
    for document in documents:
        item = dict(document)
        job = latest.get(str(item["id"]))
        if job:
            try:
                payload = json.loads(str(job.get("payload_json") or "{}"))
            except json.JSONDecodeError:
                payload = {}
            item["ingestion"] = {
                "id": job["id"],
                "status": job["status"],
                "progress": float(job["progress"]),
                "step": payload.get("step", "queued"),
                "detail": payload.get("detail", ""),
                "index_mode": payload.get("index_mode", item.get("index_mode", "simple")),
                "graph_model": payload.get("graph_model", item.get("graph_model") or ""),
                "engine": payload.get("engine", "lightrag"),
                "embedded_vectors": int(payload.get("embedded_vectors", 0) or 0),
                "estimated_chunks": int(payload.get("estimated_chunks", 0) or 0),
                "vectors_per_second": float(payload.get("vectors_per_second", 0) or 0),
                "elapsed_seconds": float(payload.get("elapsed_seconds", 0) or 0),
                "error": job.get("error"),
                "updated_at": job["updated_at"],
            }
        decorated.append(item)
    return decorated


def _queue_ingestion(
    services: AppServices,
    background_tasks: BackgroundTasks,
    document_id: str,
    *,
    indexed_only: bool,
) -> None:
    """Hand a document to whoever does the reading.

    The row's own ``status`` is the queue, so with a separate worker running there is
    nothing else to do: it polls, claims the document and reads it in its own process.
    Only the single-process setup has to run the work here, behind the response.
    """
    if not services.settings.inline_ingestion:
        return
    if indexed_only:
        background_tasks.add_task(services.document_processor.index_document, document_id)
    else:
        background_tasks.add_task(services.document_processor.process, document_id)


def _with_document_ingestion(
    services: AppServices,
    document: dict[str, Any],
) -> dict[str, Any]:
    return _with_ingestion(services, [document])[0]


@router.get("")
def list_documents(
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
    q: str = "",
    document_status: Annotated[str, Query(alias="status")] = "",
    limit: int = 20,
    offset: int = 0,
) -> dict[str, Any]:
    _require_workspace(services, workspace_id)
    page_size = max(1, min(limit, 100))
    start = max(0, offset)

    clauses = ["workspace_id = ?"]
    parameters: list[Any] = [workspace_id]
    if q.strip():
        clauses.append("filename LIKE ?")
        parameters.append(f"%{q.strip()}%")
    if document_status:
        clauses.append("status = ?")
        parameters.append(document_status)
    where = " AND ".join(clauses)

    counted = services.database.fetch_one(
        f"SELECT COUNT(*) AS total FROM documents WHERE {where}",  # noqa: S608
        tuple(parameters),
    )
    # Totals for the whole workspace stay stable so the header does not jump while filtering.
    summary = services.database.fetch_one(
        """
        SELECT COUNT(*) AS total,
               COALESCE(SUM(byte_size), 0) AS byte_size,
               COALESCE(SUM(status IN ('queued', 'processing')), 0) AS pending,
               COALESCE(SUM(status = 'ready' AND extracted_text IS NOT NULL
                            AND indexed_at IS NULL), 0) AS indexing,
               COALESCE(SUM(status IN ('failed', 'needs_ocr')), 0) AS failed
        FROM documents WHERE workspace_id = ?
        """,
        (workspace_id,),
    )
    items = _with_ingestion(
        services,
        services.database.fetch_all(
            f"SELECT * FROM documents WHERE {where} "  # noqa: S608
            "ORDER BY created_at DESC LIMIT ? OFFSET ?",
            (*parameters, page_size, start),
        ),
    )
    return {
        "items": items,
        "total": int(counted["total"]) if counted else 0,
        "limit": page_size,
        "offset": start,
        "summary": dict(summary or {}),
    }


@router.get("/search")
async def search_documents(
    q: str,
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
    limit: int = 5,
) -> list[dict[str, object]]:
    await _require_workspace_async(services, workspace_id)
    if not q.strip():
        return []
    return await services.document_processor.search(q, limit, workspace_id=workspace_id)


@router.get("/{document_id}")
def get_document(
    document_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, Any]:
    document = services.database.fetch_one("SELECT * FROM documents WHERE id = ?", (document_id,))
    if not document:
        raise HTTPException(status_code=404, detail="Document not found")
    return _with_document_ingestion(services, document)


@router.post("", status_code=status.HTTP_201_CREATED)
async def upload_document(
    background_tasks: BackgroundTasks,
    services: Annotated[AppServices, Depends(get_services)],
    file: Annotated[UploadFile, File()],
    workspace_id: Annotated[str, Form()],
    use_ocr: Annotated[bool | None, Form()] = None,
) -> dict[str, Any]:
    await _require_workspace_async(services, workspace_id)
    filename = _safe_filename(file.filename or "document")
    document_id = str(uuid4())
    target_dir = services.settings.documents_dir / document_id
    await asyncio.to_thread(target_dir.mkdir, parents=True, exist_ok=False)
    target_path = target_dir / filename
    digest = hashlib.sha256()
    byte_size = 0

    try:
        with target_path.open("wb") as output:
            while chunk := await file.read(1024 * 1024):
                byte_size += len(chunk)
                if byte_size > services.settings.max_upload_bytes:
                    raise HTTPException(
                        status_code=413,
                        detail="File exceeds configured upload limit",
                    )
                digest.update(chunk)
                output.write(chunk)
    except Exception:
        await asyncio.to_thread(shutil.rmtree, target_dir, ignore_errors=True)
        raise
    finally:
        await file.close()

    sha256 = digest.hexdigest()
    duplicate = await services.database.fetch_one_async(
        "SELECT * FROM documents WHERE workspace_id = ? AND sha256 = ?",
        (workspace_id, sha256),
    )
    if duplicate:
        await asyncio.to_thread(shutil.rmtree, target_dir, ignore_errors=True)
        return await asyncio.to_thread(_with_document_ingestion, services, duplicate)

    extension = target_path.suffix.lower()
    extracted_text: str | None = None
    document_status = "queued"
    if extension in TEXT_EXTENSIONS:
        # A text upload can be as large as the upload cap allows, so reading it whole is
        # not something the loop should be doing.
        extracted_text = await asyncio.to_thread(
            target_path.read_text,
            encoding="utf-8",
            errors="replace",
        )
        document_status = "ready"

    preferences = await asyncio.to_thread(read_app_preferences, services.database)
    index_mode = preferences.rag_mode.value
    graph_model = preferences.graph_model if index_mode == "graph" else None
    now = datetime.now(UTC).isoformat()
    await services.database.execute_async(
        """
        INSERT INTO documents(
            id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
            extracted_text, use_ocr, index_mode, graph_model, error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
        """,
        (
            document_id,
            workspace_id,
            filename,
            file.content_type,
            sha256,
            byte_size,
            document_status,
            str(target_path),
            extracted_text,
            None if use_ocr is None else int(use_ocr),
            index_mode,
            graph_model,
            now,
            now,
        ),
    )
    _queue_ingestion(services, background_tasks, document_id, indexed_only=bool(extracted_text))
    created = await services.database.fetch_one_async(
        "SELECT * FROM documents WHERE id = ?",
        (document_id,),
    )
    if not created:
        return {}
    return await asyncio.to_thread(_with_document_ingestion, services, created)


@router.post("/{document_id}/process", status_code=status.HTTP_202_ACCEPTED)
def process_document(
    document_id: str,
    background_tasks: BackgroundTasks,
    services: Annotated[AppServices, Depends(get_services)],
    use_ocr: bool | None = None,
) -> dict[str, str]:
    """Re-read a document, optionally flipping its OCR choice for this and later runs."""
    document = services.database.fetch_one("SELECT id FROM documents WHERE id = ?", (document_id,))
    if not document:
        raise HTTPException(status_code=404, detail="Document not found")
    if use_ocr is not None:
        services.database.execute(
            "UPDATE documents SET use_ocr = ? WHERE id = ?",
            (int(use_ocr), document_id),
        )
    services.database.execute(
        "UPDATE documents SET status = 'queued', error = NULL, indexed_at = NULL WHERE id = ?",
        (document_id,),
    )
    _queue_ingestion(services, background_tasks, document_id, indexed_only=False)
    return {"id": document_id, "status": "queued"}


@router.delete("/{document_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_document(
    document_id: str,
    confirmed: bool,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    if not confirmed:
        raise HTTPException(status_code=409, detail="Document deletion requires confirmation")
    if not await services.document_processor.delete(document_id):
        raise HTTPException(status_code=404, detail="Document not found")
    return Response(status_code=status.HTTP_204_NO_CONTENT)
