from __future__ import annotations

import json
from typing import Annotated

from fastapi import (
    APIRouter,
    Depends,
    File,
    HTTPException,
    Query,
    UploadFile,
    WebSocket,
    WebSocketDisconnect,
    status,
)

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.services.asr import AsrStream, AsrUnavailable
from private_ai_api.services.gpu_lease import InsufficientVram

router = APIRouter(prefix="/asr", tags=["audio"])


@router.get("/status")
async def asr_status(
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, object]:
    return services.asr.status()


@router.post("/transcribe")
async def transcribe_audio(
    services: Annotated[AppServices, Depends(get_services)],
    file: Annotated[UploadFile, File(...)],
    language: Annotated[str | None, Query(max_length=20)] = None,
) -> dict[str, object]:
    content = await file.read(services.settings.max_upload_bytes + 1)
    if len(content) > services.settings.max_upload_bytes:
        raise HTTPException(status.HTTP_413_CONTENT_TOO_LARGE, "Audio file is too large")
    try:
        return await services.asr.transcribe(
            content,
            filename=file.filename or "recording.webm",
            language=language,
        )
    except ValueError as exc:
        raise HTTPException(status.HTTP_422_UNPROCESSABLE_CONTENT, str(exc)) from exc
    except AsrUnavailable as exc:
        raise HTTPException(status.HTTP_503_SERVICE_UNAVAILABLE, str(exc)) from exc
    except InsufficientVram as exc:
        raise HTTPException(
            status.HTTP_503_SERVICE_UNAVAILABLE,
            "Not enough reserved GPU capacity for speech recognition",
        ) from exc


@router.websocket("/stream")
async def stream_audio(websocket: WebSocket) -> None:
    await websocket.accept()
    services: AppServices = websocket.app.state.services
    chunks = bytearray()
    received_bytes = 0
    language = services.settings.asr_language
    filename = "recording.webm"
    native_stream: AsrStream | None = None
    await websocket.send_json(
        {
            "type": "ready",
            "language": language,
            "formats": ["f32le", "media"],
            "streaming": services.asr.status()["streaming_available"],
        }
    )
    try:
        while True:
            message = await websocket.receive()
            if message.get("type") == "websocket.disconnect":
                return
            if payload := message.get("bytes"):
                received_bytes += len(payload)
                if received_bytes > services.settings.max_upload_bytes:
                    await websocket.send_json({"type": "error", "message": "Audio is too large"})
                    await websocket.close(code=1009)
                    return
                if native_stream is not None:
                    try:
                        result = await native_stream.feed(payload)
                    except (AsrUnavailable, ValueError) as exc:
                        await websocket.send_json({"type": "error", "message": str(exc)})
                        await websocket.close(code=1011)
                        return
                    if result["result_changed"]:
                        await websocket.send_json({"type": "partial", **result})
                    else:
                        await websocket.send_json(
                            {
                                "type": "progress",
                                "bytes": received_bytes,
                                "input_received_ms": result["input_received_ms"],
                            }
                        )
                else:
                    chunks.extend(payload)
                    await websocket.send_json({"type": "progress", "bytes": received_bytes})
                continue
            raw = message.get("text")
            if not raw:
                continue
            event = json.loads(raw)
            if event.get("type") == "config":
                if received_bytes:
                    await websocket.send_json(
                        {"type": "error", "message": "Audio configuration arrived too late"}
                    )
                    await websocket.close(code=1002)
                    return
                language = str(event.get("language") or language)[:20]
                filename = str(event.get("filename") or filename)[:200]
                if event.get("format") == "f32le":
                    if int(event.get("sample_rate") or 0) != 16000:
                        await websocket.send_json(
                            {"type": "error", "message": "Native ASR requires 16 kHz PCM"}
                        )
                        await websocket.close(code=1003)
                        return
                    try:
                        native_stream = await services.asr.open_stream(language=language)
                    except (AsrUnavailable, InsufficientVram) as exc:
                        await websocket.send_json({"type": "error", "message": str(exc)})
                        await websocket.close(code=1013)
                        return
                    await websocket.send_json(
                        {"type": "configured", "format": "f32le", "sample_rate": 16000}
                    )
            elif event.get("type") == "cancel":
                await websocket.close(code=1000)
                return
            elif event.get("type") == "commit":
                try:
                    if native_stream is not None:
                        result = await native_stream.finalize()
                        native_stream = None
                    else:
                        result = await services.asr.transcribe(
                            bytes(chunks),
                            filename=filename,
                            language=language,
                        )
                    await websocket.send_json({"type": "final", **result})
                except (AsrUnavailable, InsufficientVram, ValueError) as exc:
                    await websocket.send_json({"type": "error", "message": str(exc)})
                await websocket.close(code=1000)
                return
    except (WebSocketDisconnect, json.JSONDecodeError):
        return
    finally:
        if native_stream is not None:
            await native_stream.close()
