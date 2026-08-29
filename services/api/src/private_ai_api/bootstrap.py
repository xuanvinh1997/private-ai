"""Service construction shared by the API process and the ingestion worker.

Both processes talk to the same SQLite file and the same LightRAG working directory, so
they need identically configured services; only what they *do* with them differs. Keeping
the wiring here means a provider or embedding change cannot drift between the two.
"""

from __future__ import annotations

import shutil
from pathlib import Path

from private_ai_api.config import Settings
from private_ai_api.database import Database
from private_ai_api.dependencies import AppServices
from private_ai_api.services.app_preferences import (
    read_app_preferences,
    read_web_search_config,
)
from private_ai_api.services.asr import AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.lightrag_store import LightRagStore, default_model
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient
from private_ai_api.services.provider_registry import ProviderRegistry, ProviderRouter
from private_ai_api.services.web_search import WebSearchService


def build_services(settings: Settings, *, migrate: bool = True) -> AppServices:
    """Open the store and wire every service on top of it.

    ``migrate`` is what the second process leaves off: schema creation is idempotent, but
    only the first caller should be the one deleting purged document folders from disk.
    """
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    settings.documents_dir.mkdir(parents=True, exist_ok=True)
    database = Database(settings.database_path)
    if migrate:
        for purged in database.initialize():
            shutil.rmtree(Path(purged).parent, ignore_errors=True)
    app_preferences = read_app_preferences(database)
    gpu_leases = GpuLeaseManager(capacity_bytes=settings.gpu_capacity_bytes)
    ollama = OllamaClient(
        settings.ollama_url,
        settings.request_timeout_seconds,
        gpu_leases=gpu_leases,
        model_overhead_ratio=settings.gpu_model_overhead_ratio,
    )
    provider_registry = ProviderRegistry(
        database,
        ollama=ollama,
        ollama_url=settings.ollama_url,
        timeout=settings.request_timeout_seconds,
    )
    ai = ProviderRouter(provider_registry)
    # A provider swap usually means a different embedding model, so the stored default
    # outranks the configured one once the user has picked one.
    stored_embedding = database.fetch_one(
        "SELECT model_name FROM model_defaults WHERE task = 'embedding'"
    )
    embedding_model = (
        str(stored_embedding["model_name"]) if stored_embedding else settings.embedding_model
    )
    lightrag = LightRagStore(
        settings.data_dir,
        ai,
        embedding_model=embedding_model,
        resolve_chat_model=lambda: default_model(database, "chat"),
        resolve_graph_model=lambda: (
            read_app_preferences(database).graph_model or default_model(database, "chat")
        ),
        enabled=settings.embedding_enabled,
        embedding_batch_size=app_preferences.embedding_batch_size,
        embedding_concurrency=app_preferences.embedding_concurrency,
    )
    asr = AsrService(
        data_dir=settings.asr_dir,
        executable=settings.asr_executable,
        model_path=settings.asr_model or settings.default_asr_model_path,
        language=settings.asr_language,
        ffmpeg_executable=settings.ffmpeg_executable,
        enabled=settings.asr_enabled,
        gpu_leases=gpu_leases,
        vram_reservation_bytes=settings.asr_vram_reservation_bytes,
    )

    def vision_endpoint() -> tuple[str, str]:
        """Where markitdown should send images: the provider the user selected."""
        provider = provider_registry.active_config()
        if provider is None or provider.kind == "ollama":
            host = provider.base_url if provider else settings.ollama_url
            return f"{host.rstrip('/')}/v1", "ollama"
        return provider_registry.client_for(provider).base_url, provider.api_key

    document_processor = DocumentProcessor(
        database,
        lightrag,
        ollama_url=settings.ollama_url,
        vision_model=settings.vision_model,
        ai=ai,
        resolve_vision_endpoint=vision_endpoint,
    )
    memory_service = MemoryService(
        database,
        ai,
        embedding_model=embedding_model,
        embedding_enabled=settings.embedding_enabled,
    )
    # Resolved per call, so switching search host in settings needs no restart.
    web_search = WebSearchService(
        lambda: read_web_search_config(database),
        timeout=settings.web_search_timeout_seconds,
    )
    return AppServices(
        settings=settings,
        database=database,
        ollama=ollama,
        providers=provider_registry,
        ai=ai,
        gpu_leases=gpu_leases,
        document_processor=document_processor,
        lightrag=lightrag,
        memory_service=memory_service,
        asr=asr,
        web_search=web_search,
    )


async def close_services(services: AppServices) -> None:
    await services.asr.close()
    await services.lightrag.close()
    services.database.close()
