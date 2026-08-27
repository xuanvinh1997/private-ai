from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime
from typing import Annotated, Any
from uuid import uuid4

from fastapi import APIRouter, Depends, HTTPException, Response, status
from fastapi.responses import StreamingResponse

from private_ai_api.asr_manager import MODEL_URL, checksum_path, download
from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import DefaultModelRequest, ModelInfo, ModelState, PullRequest
from private_ai_api.services.asr import ASR_MODEL_NAME, AsrUnavailable
from private_ai_api.services.gpu_lease import InsufficientVram
from private_ai_api.services.ollama import OllamaUnavailable

router = APIRouter(prefix="/models", tags=["models"])
MODEL_TASKS = {"chat", "embedding", "vision", "asr"}


def _record_event(
    services: AppServices,
    model_name: str,
    action: str,
    event_status: str,
    detail: str | None = None,
) -> None:
    services.database.execute(
        """
        INSERT INTO model_events(id, model_name, action, status, detail, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (
            str(uuid4()),
            model_name,
            action,
            event_status,
            detail,
            datetime.now(UTC).isoformat(),
        ),
    )


def _ensure_defaults(services: AppServices, ollama_models: list[ModelInfo]) -> dict[str, str]:
    now = datetime.now(UTC).isoformat()
    language_model = next(
        (model.name for model in ollama_models if model.model_type == "language"),
        None,
    )
    embedding_model = next(
        (
            model.name
            for model in ollama_models
            if model.model_type == "embedding"
            and model.name.removesuffix(":latest")
            == services.settings.embedding_model.removesuffix(":latest")
        ),
        services.settings.embedding_model,
    )
    defaults = {
        "embedding": embedding_model,
        "asr": ASR_MODEL_NAME,
        **({"chat": language_model} if language_model else {}),
    }
    services.database.execute_many(
        "INSERT OR IGNORE INTO model_defaults(task, model_name, updated_at) VALUES (?, ?, ?)",
        ((task, model, now) for task, model in defaults.items()),
    )
    services.database.execute(
        """
        UPDATE model_defaults SET model_name = ?, updated_at = ?
        WHERE task = 'embedding' AND model_name = ?
        """,
        (embedding_model, now, services.settings.embedding_model),
    )
    return {
        str(row["task"]): str(row["model_name"])
        for row in services.database.fetch_all("SELECT task, model_name FROM model_defaults")
    }


def _asr_model_info(services: AppServices, defaults: dict[str, str]) -> ModelInfo:
    asr_status = services.asr.status()
    modified_at = (
        datetime.fromtimestamp(float(asr_status["modified_at"]), tz=UTC)
        if asr_status["modified_at"] is not None
        else None
    )
    installed = bool(asr_status["model"])
    return ModelInfo(
        name=ASR_MODEL_NAME,
        model_type="asr",
        state=(
            ModelState.LOADED
            if asr_status["native_model_loaded"]
            else ModelState.UNLOADED if installed else ModelState.FAILED
        ),
        size_bytes=int(asr_status["size_bytes"]),
        vram_bytes=(
            services.settings.asr_vram_reservation_bytes
            if asr_status["native_model_loaded"]
            else 0
        ),
        quantization="Q4_K_M",
        modified_at=modified_at,
        capabilities=["transcription", "streaming", services.settings.asr_language],
        runtime="transcribe.cpp",
        sha256=str(asr_status["sha256"]) if asr_status["sha256"] else None,
        default_for=[task for task, model in defaults.items() if model == ASR_MODEL_NAME],
        error=None if installed else "ASR model is not installed",
    )


async def _all_models(services: AppServices) -> list[ModelInfo]:
    try:
        ollama_models = await services.ollama.list_models()
    except OllamaUnavailable:
        ollama_models = []
    defaults = _ensure_defaults(services, ollama_models)
    annotated = [
        model.model_copy(
            update={
                "default_for": [
                    task for task, default_model in defaults.items() if default_model == model.name
                ]
            }
        )
        for model in ollama_models
    ]
    annotated.append(_asr_model_info(services, defaults))
    return annotated


@router.get("", response_model=list[ModelInfo])
async def list_models(
    services: Annotated[AppServices, Depends(get_services)],
) -> list[ModelInfo]:
    return await _all_models(services)


@router.get("/defaults")
async def list_model_defaults(
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, str]:
    models = await _all_models(services)
    return {
        task: model.name
        for model in models
        for task in model.default_for
    }


@router.put("/defaults/{task}")
async def select_default_model(
    task: str,
    request: DefaultModelRequest,
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, str]:
    if task not in MODEL_TASKS:
        raise HTTPException(status_code=422, detail="Unsupported model task")
    models = await _all_models(services)
    selected = next((model for model in models if model.name == request.model), None)
    if selected is None:
        raise HTTPException(status_code=404, detail="Model not found")
    expected_type = {"chat": "language", "embedding": "embedding", "asr": "asr"}.get(task)
    if expected_type and selected.model_type != expected_type:
        raise HTTPException(status_code=422, detail=f"Task {task} requires a {expected_type} model")
    if task == "vision" and "vision" not in selected.capabilities:
        raise HTTPException(status_code=422, detail="Task vision requires a vision-capable model")
    now = datetime.now(UTC).isoformat()
    services.database.execute(
        """
        INSERT INTO model_defaults(task, model_name, updated_at) VALUES (?, ?, ?)
        ON CONFLICT(task) DO UPDATE SET model_name=excluded.model_name,
                                        updated_at=excluded.updated_at
        """,
        (task, request.model, now),
    )
    _record_event(services, request.model, f"select_default:{task}", "completed")
    return {"task": task, "model": request.model}


@router.get("/events")
def list_model_events(
    services: Annotated[AppServices, Depends(get_services)],
    limit: int = 50,
) -> list[dict[str, Any]]:
    return services.database.fetch_all(
        "SELECT * FROM model_events ORDER BY created_at DESC LIMIT ?",
        (max(1, min(limit, 200)),),
    )


@router.post("/pull")
async def pull_model(
    request: PullRequest,
    services: Annotated[AppServices, Depends(get_services)],
) -> StreamingResponse:
    async def events():
        try:
            async for event in services.ollama.pull(request.name):
                yield f"data: {json.dumps(event)}\n\n"
            _record_event(services, request.name, "pull", "completed")
        except OllamaUnavailable as exc:
            _record_event(services, request.name, "pull", "failed", str(exc))
            yield f"event: error\ndata: {json.dumps({'detail': 'Ollama is not reachable'})}\n\n"

    return StreamingResponse(events(), media_type="text/event-stream")


@router.post("/{name:path}/load", status_code=status.HTTP_204_NO_CONTENT)
async def load_model(
    name: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    if name != ASR_MODEL_NAME:
        raise HTTPException(status_code=422, detail="Ollama models load when first used")
    try:
        await services.asr.load()
        _record_event(services, name, "load", "completed")
    except (AsrUnavailable, InsufficientVram) as exc:
        _record_event(services, name, "load", "failed", str(exc))
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return Response(status_code=status.HTTP_204_NO_CONTENT)


@router.post("/{name:path}/update")
async def update_model(
    name: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, str]:
    if name != ASR_MODEL_NAME:
        raise HTTPException(
            status_code=422,
            detail="Use the Ollama pull action to update this model",
        )
    try:
        await services.asr.close()
        await asyncio.to_thread(
            download,
            MODEL_URL,
            services.asr.model_path.expanduser(),
            force=True,
        )
        _record_event(services, name, "update", "completed")
    except (OSError, RuntimeError) as exc:
        _record_event(services, name, "update", "failed", str(exc))
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return {"name": name, "sha256": checksum_path(services.asr.model_path).read_text().strip()}


@router.post("/{name:path}/unload", status_code=status.HTTP_204_NO_CONTENT)
async def unload_model(
    name: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    try:
        if name == ASR_MODEL_NAME:
            await services.asr.close()
        else:
            await services.ollama.unload(name)
        _record_event(services, name, "unload", "completed")
    except (OllamaUnavailable, AsrUnavailable) as exc:
        _record_event(services, name, "unload", "failed", str(exc))
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return Response(status_code=status.HTTP_204_NO_CONTENT)


@router.delete("/{name:path}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_model(
    name: str,
    confirmed: bool,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    if not confirmed:
        raise HTTPException(status_code=409, detail="Model deletion requires explicit confirmation")
    try:
        if name == ASR_MODEL_NAME:
            await services.asr.close()
            services.asr.model_path.expanduser().unlink(missing_ok=True)
            checksum_path(services.asr.model_path.expanduser()).unlink(missing_ok=True)
        else:
            await services.ollama.delete(name)
        _record_event(services, name, "delete", "completed")
    except (OllamaUnavailable, OSError) as exc:
        _record_event(services, name, "delete", "failed", str(exc))
        raise HTTPException(status_code=503, detail=str(exc)) from exc
    return Response(status_code=status.HTTP_204_NO_CONTENT)
