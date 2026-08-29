from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager, suppress

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from private_ai_api.bootstrap import build_services, close_services
from private_ai_api.config import Settings, get_settings
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
from private_ai_api.services.provider import ProviderUnavailable


def create_app(settings: Settings | None = None) -> FastAPI:
    configured = settings or get_settings()

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        app.state.services = build_services(configured)
        # The same tool server the MCP endpoint exposes, built in-process on these very
        # services so chat can call the tools without a second process or a network hop.
        from private_ai_api.mcp_server import create_mcp_server
        from private_ai_api.services.tool_calling import McpToolBridge

        services = app.state.services
        services.tools = McpToolBridge(
            create_mcp_server(configured, services.lightrag, shared=services)
        )
        background: list[asyncio.Task[object]] = []
        # Document parsing and graph merging are CPU-bound Python. Run in this process they
        # hold the GIL for the length of a file and every request behind them waits, so the
        # desktop build hands them to private-ai-worker and leaves this loop to HTTP alone.
        if configured.inline_ingestion:
            background.append(asyncio.create_task(services.document_processor.process_pending()))
            background.append(asyncio.create_task(services.memory_service.sync_all()))
        model_inventory_task = asyncio.create_task(services.ollama.list_models())
        try:
            yield
        finally:
            for task in background:
                task.cancel()
                with suppress(asyncio.CancelledError):
                    await task
            if not model_inventory_task.done():
                model_inventory_task.cancel()
            with suppress(asyncio.CancelledError, ProviderUnavailable):
                await model_inventory_task
            await close_services(services)

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
