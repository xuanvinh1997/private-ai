from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import (
    PreferencesRecord,
    PreferencesUpdate,
    WebSearchProbe,
    WebSearchProbeResult,
)
from private_ai_api.services.app_preferences import (
    EMBEDDING_BATCH_SIZE_KEY,
    EMBEDDING_CONCURRENCY_KEY,
    GRAPH_MODEL_KEY,
    OCR_ENABLED_KEY,
    RAG_MODE_KEY,
    WEB_SEARCH_API_KEY_KEY,
    WEB_SEARCH_BACKEND_KEY,
    WEB_SEARCH_BASE_URL_KEY,
    WEB_SEARCH_ENABLED_KEY,
    WEB_SEARCH_MAX_RESULTS_KEY,
    WEB_SEARCH_MODEL_KEY,
    read_app_preferences,
    read_web_search_config,
    write_app_preference,
)
from private_ai_api.services.web_search import WebSearchConfig

router = APIRouter(prefix="/preferences", tags=["preferences"])


def _current(services: AppServices) -> PreferencesRecord:
    return PreferencesRecord.model_validate(
        read_app_preferences(services.database),
        from_attributes=True,
    )


@router.get("", response_model=PreferencesRecord)
def read_preferences(
    services: Annotated[AppServices, Depends(get_services)],
) -> PreferencesRecord:
    return _current(services)


@router.patch("", response_model=PreferencesRecord)
async def update_preferences(
    payload: PreferencesUpdate,
    services: Annotated[AppServices, Depends(get_services)],
) -> PreferencesRecord:
    if payload.ocr_enabled is not None:
        write_app_preference(
            services.database,
            OCR_ENABLED_KEY,
            "1" if payload.ocr_enabled else "0",
        )
    if payload.rag_mode is not None:
        write_app_preference(services.database, RAG_MODE_KEY, payload.rag_mode.value)
    if payload.graph_model is not None:
        write_app_preference(services.database, GRAPH_MODEL_KEY, payload.graph_model.strip())
    if payload.embedding_batch_size is not None:
        write_app_preference(
            services.database,
            EMBEDDING_BATCH_SIZE_KEY,
            str(payload.embedding_batch_size),
        )
    if payload.embedding_concurrency is not None:
        write_app_preference(
            services.database,
            EMBEDDING_CONCURRENCY_KEY,
            str(payload.embedding_concurrency),
        )
    if payload.web_search_enabled is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_ENABLED_KEY,
            "1" if payload.web_search_enabled else "0",
        )
    if payload.web_search_backend is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_BACKEND_KEY,
            payload.web_search_backend.value,
        )
    if payload.web_search_base_url is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_BASE_URL_KEY,
            payload.web_search_base_url.strip(),
        )
    if payload.web_search_api_key is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_API_KEY_KEY,
            payload.web_search_api_key.strip(),
        )
    if payload.web_search_model is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_MODEL_KEY,
            payload.web_search_model.strip(),
        )
    if payload.web_search_max_results is not None:
        write_app_preference(
            services.database,
            WEB_SEARCH_MAX_RESULTS_KEY,
            str(payload.web_search_max_results),
        )
    current = _current(services)
    if payload.embedding_batch_size is not None or payload.embedding_concurrency is not None:
        await services.lightrag.configure_indexing(
            batch_size=current.embedding_batch_size,
            concurrency=current.embedding_concurrency,
        )
    return current


@router.post("/web-search/probe", response_model=WebSearchProbeResult)
async def probe_web_search(
    payload: WebSearchProbe,
    services: Annotated[AppServices, Depends(get_services)],
) -> WebSearchProbeResult:
    """Run one throwaway query, so a bad host shows up in settings and not mid-chat."""
    stored = read_web_search_config(services.database)
    draft = WebSearchConfig(
        backend=payload.backend.value,
        base_url=payload.base_url.strip() or stored.base_url,
        # A blank key means "keep using the saved one", so testing needs no retyping.
        api_key=payload.api_key.strip() or stored.api_key,
        model=payload.model.strip() or stored.model,
        max_results=stored.max_results,
    )
    return WebSearchProbeResult(**await services.web_search.probe(draft))
