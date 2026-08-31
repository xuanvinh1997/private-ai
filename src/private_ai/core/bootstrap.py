"""Service construction, shared by the desktop app, the ingestion worker and every MCP server.

All of them talk to the same SQLite file and the same LightRAG working directory, so they
need identically configured services; only what they *do* with them differs. Keeping the
wiring in one place means a provider or embedding change cannot drift between processes.

``build_services`` is synchronous because every constructor here is. The two things that
genuinely need a running loop — mounting the MCP servers and compiling the agent graph —
live in ``start_services``, which the UI awaits once its qasync loop is up.
"""

from __future__ import annotations

import asyncio
import logging
import shutil
from contextlib import suppress
from pathlib import Path
from typing import Any, cast

from langchain_core.embeddings import Embeddings

from private_ai.agent.skills.registry import SkillRegistry
from private_ai.asr.service import AsrService
from private_ai.config import Settings, get_settings
from private_ai.core.artifacts import ArtifactStore
from private_ai.core.database import Database
from private_ai.core.file_access import FileAccessService
from private_ai.core.gpu_lease import GpuLeaseManager
from private_ai.core.preferences import read_app_preferences, read_web_search_config
from private_ai.core.services import AppServices
from private_ai.llm import ProviderUnavailable
from private_ai.llm.registry import ProviderRegistry
from private_ai.llm.router import ModelRouter
from private_ai.memory.store import MemoryStore
from private_ai.rag.ingestion.pipeline import IngestionPipeline
from private_ai.rag.stores.graph_store import GraphStore
from private_ai.rag.stores.sqlite_vectorstore import SqliteVectorStore
from private_ai.rag.strategies.registry import StrategyRegistry
from private_ai.rag.web_search import WebSearchService

logger = logging.getLogger("private_ai.bootstrap")


class _LazyEmbeddings(Embeddings):
    """Resolves the embeddings client on first use instead of at wiring time.

    The vector store is the only service that wants a concrete ``Embeddings`` in its
    constructor; the graph store and the memory store ask the router per call. Without
    this, a machine with every provider deleted could not build its services at all —
    the app would fail to start on the one screen that could fix the problem.
    """

    __slots__ = ("_model", "_router")

    def __init__(self, router: ModelRouter, model: str) -> None:
        self._router = router
        self._model = model

    def _resolved(self) -> Embeddings:
        return self._router.embeddings(self._model)

    def embed_documents(self, texts: list[str]) -> list[list[float]]:
        return self._resolved().embed_documents(texts)

    def embed_query(self, text: str) -> list[float]:
        return self._resolved().embed_query(text)

    async def aembed_documents(self, texts: list[str]) -> list[list[float]]:
        return await self._resolved().aembed_documents(texts)

    async def aembed_query(self, text: str) -> list[float]:
        return await self._resolved().aembed_query(text)


def resolve_embedding_model(database: Database, settings: Settings) -> str:
    """A provider swap usually means a different embedding model.

    Once the user has picked one it outranks the configured default, because the vectors
    already on disk were produced by it and rows from another model are skipped, not
    rescored.
    """
    row = database.fetch_one("SELECT model_name FROM model_defaults WHERE task = 'embedding'")
    stored = str(row["model_name"]).strip() if row else ""
    return stored or settings.embedding_model


def build_services(settings: Settings | None = None, *, migrate: bool = True) -> AppServices:
    """Open the store and wire every service on top of it.

    ``migrate`` is what the second process leaves off: schema creation is idempotent, but
    only the first caller should be the one deleting purged document folders from disk.
    The desktop app migrates; ``private-ai-worker`` attaches to what the app created.
    """
    configured = settings or get_settings()
    configured.data_dir.mkdir(parents=True, exist_ok=True)
    configured.documents_dir.mkdir(parents=True, exist_ok=True)

    database = Database(configured.database_path)
    if migrate:
        for purged in database.initialize():
            # ``initialize`` returns the file of every document it dropped; the folder
            # around it is ours and holds nothing else.
            shutil.rmtree(Path(purged).parent, ignore_errors=True)
    preferences = read_app_preferences(database)

    # One GpuLeaseManager per process, and one GraphStore: a second lease manager would
    # double-count every VRAM reservation, and a second graph store would open the same
    # LightRAG files twice.
    gpu_leases = GpuLeaseManager(capacity_bytes=configured.gpu_capacity_bytes)
    providers = ProviderRegistry(database, settings=configured)
    models = ModelRouter(
        providers,
        gpu_leases=gpu_leases,
        settings=configured,
        database=database,
    )

    embedding_model = resolve_embedding_model(database, configured)
    vectors = SqliteVectorStore(
        database,
        _LazyEmbeddings(models, embedding_model),
        embedding_model=embedding_model,
    )
    graph = GraphStore(
        configured.data_dir,
        models,
        embedding_model=embedding_model,
        # Read per call, not captured: the user can change the chat or graph model from
        # settings and the next indexing run has to see it without a restart.
        resolve_chat_model=lambda: models.default_model("chat"),
        resolve_graph_model=lambda: (
            read_app_preferences(database).graph_model or models.default_model("chat")
        ),
        enabled=configured.embedding_enabled,
        embedding_batch_size=preferences.embedding_batch_size,
        embedding_concurrency=preferences.embedding_concurrency,
    )
    ingestion = IngestionPipeline(database, vectors, graph, models, configured)
    # Resolved per call for the same reason, so switching search host needs no restart.
    web_search = WebSearchService(
        lambda: read_web_search_config(database),
        timeout=configured.web_search_timeout_seconds,
    )
    memory = MemoryStore(
        database,
        models,
        embedding_model=embedding_model,
        enabled=configured.embedding_enabled,
    )
    files = FileAccessService(
        database,
        roots=configured.file_root_paths,
        # The MCP token is the key to every other tool, so it is never readable.
        protected=(configured.mcp_token_path,),
        max_read_bytes=configured.file_read_max_bytes,
    )
    skills = SkillRegistry(database, configured)
    artifacts = ArtifactStore(configured.artifacts_dir)
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

    services = AppServices(
        settings=configured,
        database=database,
        providers=providers,
        models=models,
        gpu_leases=gpu_leases,
        vectors=vectors,
        graph=graph,
        # Every strategy reads the container it lives in, so the registry cannot exist
        # before AppServices does. It is the one field filled in straight after.
        strategies=cast(StrategyRegistry, None),
        ingestion=ingestion,
        web_search=web_search,
        memory=memory,
        files=files,
        skills=skills,
        artifacts=artifacts,
        asr=asr,
    )
    services.strategies = StrategyRegistry(services)
    return services


async def start_services(services: AppServices) -> None:
    """The half of startup that needs a running loop.

    Call once, from the process that owns the event loop. The worker and the standalone
    MCP servers skip it: neither runs the agent, and neither should mount a second hub.
    """
    from private_ai.mcp.client import McpHub

    services.mcp = McpHub(services)
    await services.mcp.start()
    # Built after the hub, because the graph binds the hub's tools when it compiles.
    from private_ai.agent.runner import AgentRunner

    services.agent = AgentRunner(services)
    await asyncio.to_thread(services.skills.refresh)
    # Warming the model list costs one provider round trip and every screen that names a
    # model wants it. Backgrounded so a provider that is down delays nothing.
    services._tasks.append(asyncio.create_task(_warm_model_inventory(services)))


async def _warm_model_inventory(services: AppServices) -> None:
    with suppress(asyncio.CancelledError, ProviderUnavailable, OSError):
        await services.models.list_models()


async def close_services(services: AppServices) -> None:
    """Tear down in reverse dependency order, and never let teardown raise.

    A provider that has gone away is the ordinary case at shutdown — the user quit because
    Ollama died — so ``ProviderUnavailable`` here is noise, not news.
    """
    for task in services._tasks:
        task.cancel()
    for task in services._tasks:
        with suppress(asyncio.CancelledError, Exception):
            await task
    services._tasks.clear()

    if services.mcp is not None:
        await _closing("mcp", services.mcp.close())
        services.mcp = None
    services.agent = None
    await _closing("asr", services.asr.close())
    await _closing("graph", services.graph.close())
    services.database.close()


async def _closing(name: str, awaitable: Any) -> None:
    try:
        await awaitable
    except (asyncio.CancelledError, ProviderUnavailable):
        pass
    except Exception:  # pragma: no cover - shutdown must not mask the real exit reason
        logger.exception("Lỗi khi đóng dịch vụ %s", name)
