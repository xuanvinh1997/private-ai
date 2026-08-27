from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends

from private_ai_api.dependencies import AppServices, get_services

router = APIRouter(tags=["system"])


@router.get("/health")
async def health(services: Annotated[AppServices, Depends(get_services)]) -> dict[str, object]:
    ollama_available = await services.ollama.health()
    neo4j_available = await services.graph_store.health()
    asr_available = await services.asr.health()
    return {
        "status": "ok",
        "platform": services.settings.platform_name,
        "services": {
            "api": "online",
            "database": "online",
            "ollama": "online" if ollama_available else "offline",
            "neo4j": "online" if neo4j_available else "offline",
            "asr": "online" if asr_available else "offline",
        },
        "gpu": services.gpu_leases.snapshot(),
    }
