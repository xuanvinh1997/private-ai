from __future__ import annotations

import hashlib
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
    Response,
    UploadFile,
    status,
)

from private_ai_api.dependencies import AppServices, get_services

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


@router.get("")
def list_documents(
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> list[dict[str, Any]]:
    _require_workspace(services, workspace_id)
    return services.database.fetch_all(
        "SELECT * FROM documents WHERE workspace_id = ? ORDER BY created_at DESC",
        (workspace_id,),
    )


@router.get("/search")
async def search_documents(
    q: str,
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
    limit: int = 5,
) -> list[dict[str, object]]:
    _require_workspace(services, workspace_id)
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
    return document


@router.post("", status_code=status.HTTP_201_CREATED)
async def upload_document(
    background_tasks: BackgroundTasks,
    services: Annotated[AppServices, Depends(get_services)],
    file: Annotated[UploadFile, File()],
    workspace_id: Annotated[str, Form()],
) -> dict[str, Any]:
    _require_workspace(services, workspace_id)
    filename = _safe_filename(file.filename or "document")
    document_id = str(uuid4())
    target_dir = services.settings.documents_dir / document_id
    target_dir.mkdir(parents=True, exist_ok=False)
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
        shutil.rmtree(target_dir, ignore_errors=True)
        raise
    finally:
        await file.close()

    sha256 = digest.hexdigest()
    duplicate = services.database.fetch_one(
        "SELECT * FROM documents WHERE workspace_id = ? AND sha256 = ?",
        (workspace_id, sha256),
    )
    if duplicate:
        shutil.rmtree(target_dir, ignore_errors=True)
        return duplicate

    extension = target_path.suffix.lower()
    extracted_text: str | None = None
    document_status = "queued"
    if extension in TEXT_EXTENSIONS:
        extracted_text = target_path.read_text(encoding="utf-8", errors="replace")
        document_status = "ready"

    now = datetime.now(UTC).isoformat()
    services.database.execute(
        """
        INSERT INTO documents(
            id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
            extracted_text, error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
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
            now,
            now,
        ),
    )
    if extracted_text:
        services.document_processor.index_text(document_id, extracted_text)
        background_tasks.add_task(services.document_processor.embed_document, document_id)
    if document_status == "queued":
        background_tasks.add_task(services.document_processor.process, document_id)
    return services.database.fetch_one("SELECT * FROM documents WHERE id = ?", (document_id,)) or {}


@router.post("/{document_id}/process", status_code=status.HTTP_202_ACCEPTED)
def process_document(
    document_id: str,
    background_tasks: BackgroundTasks,
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, str]:
    document = services.database.fetch_one("SELECT id FROM documents WHERE id = ?", (document_id,))
    if not document:
        raise HTTPException(status_code=404, detail="Document not found")
    services.database.execute(
        "UPDATE documents SET status = 'queued', error = NULL WHERE id = ?",
        (document_id,),
    )
    background_tasks.add_task(services.document_processor.process, document_id)
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
