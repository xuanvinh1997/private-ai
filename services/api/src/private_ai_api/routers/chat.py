from __future__ import annotations

from typing import Annotated, Any

from fastapi import APIRouter, Depends, HTTPException

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import ChatRequest
from private_ai_api.services.gpu_lease import InsufficientVram
from private_ai_api.services.ollama import OllamaUnavailable

router = APIRouter(prefix="/chat", tags=["chat"])


@router.post("")
async def chat(
    request: ChatRequest,
    services: Annotated[AppServices, Depends(get_services)],
) -> dict[str, Any]:
    if request.stream:
        raise HTTPException(status_code=422, detail="Use the WebSocket endpoint for streaming chat")
    try:
        return await services.ollama.chat(request)
    except OllamaUnavailable as exc:
        raise HTTPException(status_code=503, detail="Ollama is not reachable") from exc
    except InsufficientVram as exc:
        raise HTTPException(status_code=503, detail="Not enough reserved GPU capacity") from exc
