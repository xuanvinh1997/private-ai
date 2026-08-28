from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends

from private_ai_api.config import is_unified_memory, total_memory_bytes
from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.services.provider import runs_on_device

router = APIRouter(tags=["system"])


@router.get("/health/live")
async def liveness() -> dict[str, str]:
    """Liveness only: launchers poll this while the runtime services are still waking up."""
    return {"status": "ok"}


@router.get("/health")
async def health(services: Annotated[AppServices, Depends(get_services)]) -> dict[str, object]:
    provider = services.providers.active_config()
    # Polling the local server also refreshes the GPU leases behind the resource panel, so it
    # runs even when the active provider is a remote endpoint.
    local_runtime_available = await services.ollama.health()
    if provider is None:
        provider_available = False
    elif provider.builtin:
        provider_available = local_runtime_available
    else:
        provider_available = await services.ai.health()
    index_available = await services.lightrag.health()
    asr_available = await services.asr.health()
    # The seeded local record can be repointed at WSL2 or another machine, so trust the URL.
    on_device = provider is not None and runs_on_device(provider.base_url)
    return {
        "status": "ok",
        "platform": services.settings.platform_name,
        "services": {
            "api": "online",
            "database": "online",
            "provider": "not_configured"
            if provider is None
            else "online"
            if provider_available
            else "offline",
            "local_runtime": "online" if local_runtime_available else "offline",
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
            "on_device": on_device,
        },
        "gpu": {
            **services.gpu_leases.snapshot(),
            "unified_memory": is_unified_memory(),
            "total_memory_bytes": total_memory_bytes(),
        },
    }
