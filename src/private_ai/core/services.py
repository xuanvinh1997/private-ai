"""The single service container.

Everything the application can do hangs off ``AppServices``. The desktop UI, the
ingestion worker and every MCP server build one of these and then talk to the same
objects — there is no HTTP boundary and no second copy of any stateful service.

Two of these fields are singletons for a reason worth remembering: a second
``GpuLeaseManager`` would double-count every VRAM reservation, and a second graph
store would open the same files twice.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.agent.runner import AgentRunner
    from private_ai.agent.skills.registry import SkillRegistry
    from private_ai.asr.service import AsrService
    from private_ai.config import Settings
    from private_ai.core.database import Database
    from private_ai.core.file_access import FileAccessService
    from private_ai.core.gpu_lease import GpuLeaseManager
    from private_ai.llm.registry import ProviderRegistry
    from private_ai.llm.router import ModelRouter
    from private_ai.mcp.client import McpHub
    from private_ai.memory.store import MemoryStore
    from private_ai.rag.ingestion.pipeline import IngestionPipeline
    from private_ai.rag.stores.graph_store import GraphStore
    from private_ai.rag.stores.sqlite_vectorstore import SqliteVectorStore
    from private_ai.rag.strategies.registry import StrategyRegistry
    from private_ai.rag.web_search import WebSearchService


@dataclass(slots=True)
class AppServices:
    """Live services, built once per process by ``bootstrap.build_services``."""

    settings: Settings
    database: Database

    # Models
    providers: ProviderRegistry
    models: ModelRouter
    gpu_leases: GpuLeaseManager

    # Retrieval
    vectors: SqliteVectorStore
    graph: GraphStore
    strategies: StrategyRegistry
    ingestion: IngestionPipeline
    web_search: WebSearchService

    # Knowledge and capability
    memory: MemoryStore
    files: FileAccessService
    skills: SkillRegistry

    # Speech
    asr: AsrService

    # Filled in after the rest exist: the MCP hub mounts servers that close over
    # these very services, so it cannot be constructed alongside them.
    mcp: McpHub | None = None
    agent: AgentRunner | None = None

    # Background tasks owned by the process that built this container.
    _tasks: list = field(default_factory=list, repr=False)
