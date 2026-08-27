from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, Response, status

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import (
    ProviderCreate,
    ProviderProbe,
    ProviderProbeResult,
    ProviderRecord,
    ProviderUpdate,
)
from private_ai_api.services.ollama import OllamaClient
from private_ai_api.services.openai_compat import OpenAICompatClient
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import (
    ProviderConfig,
    ProviderRegistry,
    UnknownProvider,
)

router = APIRouter(prefix="/providers", tags=["providers"])
PROBE_MODEL_PREVIEW = 12


def _records(registry: ProviderRegistry) -> list[ProviderRecord]:
    active_id = registry.active_id()
    return [
        ProviderRecord(**config.public(active=config.id == active_id))
        for config in registry.list_configs()
    ]


def _record(registry: ProviderRegistry, config: ProviderConfig) -> ProviderRecord:
    return ProviderRecord(**config.public(active=registry.active_id() == config.id))


def _lookup(registry: ProviderRegistry, provider_id: str) -> ProviderConfig:
    try:
        return registry.get(provider_id)
    except UnknownProvider as exc:
        raise HTTPException(status_code=404, detail="Provider not found") from exc


@router.get("", response_model=list[ProviderRecord])
def list_providers(
    services: Annotated[AppServices, Depends(get_services)],
) -> list[ProviderRecord]:
    return _records(services.providers)


@router.post("", response_model=ProviderRecord, status_code=status.HTTP_201_CREATED)
def create_provider(
    payload: ProviderCreate,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProviderRecord:
    try:
        config = services.providers.create(
            name=payload.name,
            kind=payload.kind.value,
            base_url=payload.base_url,
            api_key=payload.api_key,
            enabled=payload.enabled,
        )
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return _record(services.providers, config)


@router.patch("/{provider_id}", response_model=ProviderRecord)
def update_provider(
    provider_id: str,
    payload: ProviderUpdate,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProviderRecord:
    _lookup(services.providers, provider_id)
    try:
        config = services.providers.update(
            provider_id,
            name=payload.name,
            base_url=payload.base_url,
            api_key=payload.api_key,
            enabled=payload.enabled,
        )
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return _record(services.providers, config)


@router.post("/{provider_id}/activate", response_model=ProviderRecord)
def activate_provider(
    provider_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProviderRecord:
    _lookup(services.providers, provider_id)
    try:
        config = services.providers.activate(provider_id)
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return _record(services.providers, config)


@router.post("/probe", response_model=ProviderProbeResult)
async def probe_draft_provider(
    payload: ProviderProbe,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProviderProbeResult:
    """Check an unsaved connection, so a wrong URL or key never becomes the active one."""
    timeout = services.settings.request_timeout_seconds
    try:
        client: OllamaClient | OpenAICompatClient = (
            OllamaClient(payload.base_url, timeout)
            if payload.kind.value == "ollama"
            else OpenAICompatClient(payload.base_url, payload.api_key, timeout)
        )
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return await _probe(client)


@router.post("/{provider_id}/probe", response_model=ProviderProbeResult)
async def probe_provider(
    provider_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProviderProbeResult:
    config = _lookup(services.providers, provider_id)
    return await _probe(services.providers.client_for(config))


@router.delete("/{provider_id}", status_code=status.HTTP_204_NO_CONTENT)
def delete_provider(
    provider_id: str,
    confirmed: bool,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    if not confirmed:
        raise HTTPException(status_code=409, detail="Provider removal requires confirmation")
    _lookup(services.providers, provider_id)
    try:
        services.providers.delete(provider_id)
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return Response(status_code=status.HTTP_204_NO_CONTENT)


async def _probe(client: OllamaClient | OpenAICompatClient) -> ProviderProbeResult:
    try:
        models = await client.list_models()
    except ProviderUnavailable as exc:
        return ProviderProbeResult(reachable=False, detail=str(exc))
    return ProviderProbeResult(
        reachable=True,
        model_count=len(models),
        models=[model.name for model in models[:PROBE_MODEL_PREVIEW]],
    )
