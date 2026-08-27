from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager, suppress

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from private_ai_api.config import Settings, get_settings
from private_ai_api.database import Database
from private_ai_api.dependencies import AppServices
from private_ai_api.routers import audio, chat, documents, health, memory, models, workspaces
from private_ai_api.services.asr import AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.graph_store import GraphStore
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient, OllamaUnavailable


def create_app(settings: Settings | None = None) -> FastAPI:
    configured = settings or get_settings()

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        configured.data_dir.mkdir(parents=True, exist_ok=True)
        configured.documents_dir.mkdir(parents=True, exist_ok=True)
        database = Database(configured.database_path)
        database.initialize()
        gpu_leases = GpuLeaseManager(capacity_bytes=configured.gpu_capacity_bytes)
        ollama = OllamaClient(
            configured.ollama_url,
            configured.request_timeout_seconds,
            gpu_leases=gpu_leases,
            model_overhead_ratio=configured.gpu_model_overhead_ratio,
        )
        graph_store = GraphStore(
            database,
            url=configured.neo4j_url,
            user=configured.neo4j_user,
            password=configured.resolved_neo4j_password(),
            neo4j_database=configured.neo4j_database,
            enabled=configured.neo4j_enabled,
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
        document_processor = DocumentProcessor(
            database,
            ollama,
            embedding_model=configured.embedding_model,
            embedding_enabled=configured.embedding_enabled,
            graph_store=graph_store,
            graph_entity_model=configured.graph_entity_model,
            ollama_url=configured.ollama_url,
            vision_model=configured.vision_model,
        )
        memory_service = MemoryService(
            database,
            ollama,
            graph_store,
            embedding_model=configured.embedding_model,
            embedding_enabled=configured.embedding_enabled,
        )
        app.state.services = AppServices(
            settings=configured,
            database=database,
            ollama=ollama,
            gpu_leases=gpu_leases,
            document_processor=document_processor,
            graph_store=graph_store,
            memory_service=memory_service,
            asr=asr,
        )
        graph_task = asyncio.create_task(graph_store.initialize())
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
            with suppress(asyncio.CancelledError):
                await graph_task
            if not model_inventory_task.done():
                model_inventory_task.cancel()
            with suppress(asyncio.CancelledError, OllamaUnavailable):
                await model_inventory_task
            await asr.close()
            await graph_store.close()

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
    app.include_router(chat.router, prefix="/api/v1")
    app.include_router(memory.router, prefix="/api/v1")
    app.include_router(documents.router, prefix="/api/v1")
    app.include_router(workspaces.router, prefix="/api/v1")
    app.include_router(audio.router, prefix="/api/v1")
    if configured.frontend_dist.is_dir():
        app.mount("/", StaticFiles(directory=configured.frontend_dist, html=True), name="web")
    return app


app = create_app()


def run() -> None:
    settings = get_settings()
    uvicorn.run("private_ai_api.main:app", host=settings.host, port=settings.port, reload=False)
