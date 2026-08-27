from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends

from private_ai_api.config import is_unified_memory, total_memory_bytes
from private_ai_api.dependencies import AppServices, get_services

router = APIRouter(tags=["system"])


@router.get("/health")
async def health(services: Annotated[AppServices, Depends(get_services)]) -> dict[str, object]:
    provider = services.providers.active_config()
    ollama_available = await services.ollama.health()
    if provider is None:
        provider_available = False
    elif provider.builtin:
        provider_available = ollama_available
    else:
        provider_available = await services.ai.health()
    index_available = await services.lightrag.health()
    asr_available = await services.asr.health()
    return {
        "status": "ok",
        "platform": services.settings.platform_name,
        "services": {
            "api": "online",
            "database": "online",
            "ollama": "online" if ollama_available else "offline",
            "provider": "online" if provider_available else "not_configured"
            if provider is None
            else "offline",
            "knowledge_graph": "online" if index_available else "not_configured",
            "asr": "online" if asr_available else "offline",
        },
        "provider": None
        if provider is None
        else {
            "id": provider.id,
            "name": provider.name,
            "kind": provider.kind,
            "base_url": provider.base_url,
            "builtin": provider.builtin,
        },
        "gpu": {
            **services.gpu_leases.snapshot(),
            "unified_memory": is_unified_memory(),
            "total_memory_bytes": total_memory_bytes(),
        },
    }
