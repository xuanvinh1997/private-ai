"""One place that knows every strategy by name.

The UI, the agent and each MCP server all address strategies by the string the user
picked in preferences, so name resolution lives here rather than in a dict copied into
three modules.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.auto import AutoStrategy
from private_ai.rag.strategies.base import Strategy
from private_ai.rag.strategies.graph import GraphStrategy
from private_ai.rag.strategies.hybrid import HybridStrategy
from private_ai.rag.strategies.keyword import KeywordStrategy
from private_ai.rag.strategies.summary import SummaryStrategy
from private_ai.rag.strategies.vector import VectorStrategy
from private_ai.rag.strategies.web import WebStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

STRATEGY_TYPES: dict[str, type[Strategy]] = {
    RetrievalStrategyName.VECTOR.value: VectorStrategy,
    RetrievalStrategyName.KEYWORD.value: KeywordStrategy,
    RetrievalStrategyName.HYBRID.value: HybridStrategy,
    RetrievalStrategyName.GRAPH.value: GraphStrategy,
    RetrievalStrategyName.SUMMARY.value: SummaryStrategy,
    RetrievalStrategyName.WEB.value: WebStrategy,
    RetrievalStrategyName.AUTO.value: AutoStrategy,
}


class StrategyRegistry:
    def __init__(self, services: AppServices) -> None:
        self.services = services
        # Built on first use: an MCP server that only ever serves `rag.keyword.search`
        # has no reason to construct a summary strategy.
        self._instances: dict[str, Strategy] = {}

    def get(self, name: str | RetrievalStrategyName) -> Strategy:
        key = str(getattr(name, "value", name)).strip().lower()
        factory = STRATEGY_TYPES.get(key)
        if factory is None:
            valid = ", ".join(self.names())
            raise KeyError(f"Chiến lược truy hồi không tồn tại: {name!r}. Hợp lệ: {valid}")
        instance = self._instances.get(key)
        if instance is None:
            instance = factory(self.services)
            self._instances[key] = instance
        return instance

    def names(self) -> list[str]:
        return list(STRATEGY_TYPES)

    def all(self) -> list[Strategy]:
        return [self.get(name) for name in self.names()]

    async def retrieve(
        self,
        query: str,
        *,
        workspace_id: str,
        strategy: str | RetrievalStrategyName = RetrievalStrategyName.AUTO,
        limit: int = 5,
        **options: Any,
    ) -> list[Document]:
        name = str(getattr(strategy, "value", strategy)).strip().lower()
        return await self.get(name or RetrievalStrategyName.AUTO.value).retrieve(
            query,
            workspace_id=workspace_id,
            limit=limit,
            **options,
        )
