from __future__ import annotations

import asyncio
import shutil
from contextlib import asynccontextmanager, suppress
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from private_ai_api.config import Settings, get_settings
from private_ai_api.database import Database
from private_ai_api.dependencies import AppServices
from private_ai_api.routers import (
    audio,
    chat,
    documents,
    graph,
    health,
    memory,
    models,
    preferences,
    profiles,
    providers,
    workspaces,
)
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
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRegistry, ProviderRouter
from private_ai_api.services.tool_calling import McpToolBridge
from private_ai_api.services.web_search import WebSearchService


def create_app(settings: Settings | None = None) -> FastAPI:
    configured = settings or get_settings()

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        configured.data_dir.mkdir(parents=True, exist_ok=True)
        configured.documents_dir.mkdir(parents=True, exist_ok=True)
        database = Database(configured.database_path)
        for purged in database.initialize():
            shutil.rmtree(Path(purged).parent, ignore_errors=True)
        app_preferences = read_app_preferences(database)
        gpu_leases = GpuLeaseManager(capacity_bytes=configured.gpu_capacity_bytes)
        ollama = OllamaClient(
            configured.ollama_url,
            configured.request_timeout_seconds,
            gpu_leases=gpu_leases,
            model_overhead_ratio=configured.gpu_model_overhead_ratio,
        )
        provider_registry = ProviderRegistry(
            database,
            ollama=ollama,
            ollama_url=configured.ollama_url,
            timeout=configured.request_timeout_seconds,
        )
        ai = ProviderRouter(provider_registry)
        # A provider swap usually means a different embedding model, so the stored default
        # outranks the configured one once the user has picked one.
        stored_embedding = database.fetch_one(
            "SELECT model_name FROM model_defaults WHERE task = 'embedding'"
        )
        embedding_model = (
            str(stored_embedding["model_name"]) if stored_embedding else configured.embedding_model
        )
        lightrag = LightRagStore(
            configured.data_dir,
            ai,
            embedding_model=embedding_model,
            resolve_chat_model=lambda: default_model(database, "chat"),
            resolve_graph_model=lambda: (
                read_app_preferences(database).graph_model or default_model(database, "chat")
            ),
            enabled=configured.embedding_enabled,
            embedding_batch_size=app_preferences.embedding_batch_size,
            embedding_concurrency=app_preferences.embedding_concurrency,
        )
        asr = AsrService(
            data_dir=configured.asr_dir,
            executable=configured.asr_executable,
            model_path=configured.asr_model or configured.default_asr_model_path,
            language=configured.asr_language,
            ffmpeg_executable=configured.ffmpeg_executable,
            enabled=configured.asr_enabled,
            gpu_leases=gpu_leases,
            vram_reservation_bytes=configured.asr_vram_reservation_bytes,
        )

        def vision_endpoint() -> tuple[str, str]:
            """Where markitdown should send images: the provider the user selected."""
            provider = provider_registry.active_config()
            if provider is None or provider.kind == "ollama":
                host = provider.base_url if provider else configured.ollama_url
                return f"{host.rstrip('/')}/v1", "ollama"
            return provider_registry.client_for(provider).base_url, provider.api_key

        document_processor = DocumentProcessor(
            database,
            lightrag,
            ollama_url=configured.ollama_url,
            vision_model=configured.vision_model,
            ai=ai,
            resolve_vision_endpoint=vision_endpoint,
        )
        memory_service = MemoryService(
            database,
            ai,
            embedding_model=embedding_model,
            embedding_enabled=configured.embedding_enabled,
        )
        # Resolved per call, so switching search host in settings needs no restart.
        web_search = WebSearchService(
            lambda: read_web_search_config(database),
            timeout=configured.web_search_timeout_seconds,
        )
        app.state.services = AppServices(
            settings=configured,
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
        # The same tool server the MCP endpoint exposes, built in-process on these very
        # services so chat can call the tools without a second process or a network hop.
        from private_ai_api.mcp_server import create_mcp_server

        app.state.services.tools = McpToolBridge(
            create_mcp_server(configured, lightrag, shared=app.state.services)
        )
        ingestion_task = asyncio.create_task(document_processor.process_pending())
        memory_task = asyncio.create_task(memory_service.sync_all())
        model_inventory_task = asyncio.create_task(ollama.list_models())
        try:
            yield
        finally:
            ingestion_task.cancel()
            with suppress(asyncio.CancelledError):
                await ingestion_task
            if not memory_task.done():
                memory_task.cancel()
            with suppress(asyncio.CancelledError):
                await memory_task
            if not model_inventory_task.done():
                model_inventory_task.cancel()
            with suppress(asyncio.CancelledError, ProviderUnavailable):
                await model_inventory_task
            await asr.close()
            await lightrag.close()

    app = FastAPI(
        title=configured.app_name,
        version="0.1.0",
        lifespan=lifespan,
    )
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["http://127.0.0.1:5173", "http://localhost:5173"],
        allow_credentials=False,
        allow_methods=["*"],
        allow_headers=["*"],
    )
    app.include_router(health.router, prefix="/api/v1")
    app.include_router(models.router, prefix="/api/v1")
    app.include_router(providers.router, prefix="/api/v1")
    app.include_router(preferences.router, prefix="/api/v1")
    app.include_router(profiles.router, prefix="/api/v1")
    app.include_router(chat.router, prefix="/api/v1")
    app.include_router(memory.router, prefix="/api/v1")
    app.include_router(documents.router, prefix="/api/v1")
    app.include_router(workspaces.router, prefix="/api/v1")
    app.include_router(graph.router, prefix="/api/v1")
    app.include_router(audio.router, prefix="/api/v1")
    if configured.frontend_dist.is_dir():
        app.mount("/", StaticFiles(directory=configured.frontend_dist, html=True), name="web")
    return app


app = create_app()


def run() -> None:
    settings = get_settings()
    uvicorn.run("private_ai_api.main:app", host=settings.host, port=settings.port, reload=False)
