"""Retrieval strategies — one object per way of finding a passage, one MCP server each."""

from __future__ import annotations

from private_ai.rag.strategies.auto import AutoStrategy
from private_ai.rag.strategies.base import (
    MAX_RESULTS,
    RRF_K,
    UNTRUSTED_NOTICE,
    Strategy,
    StrategyRetriever,
    deduplicate,
    reciprocal_rank_fusion,
    stamp,
)
from private_ai.rag.strategies.graph import GraphStrategy
from private_ai.rag.strategies.hybrid import HybridStrategy
from private_ai.rag.strategies.keyword import KeywordStrategy
from private_ai.rag.strategies.registry import STRATEGY_TYPES, StrategyRegistry
from private_ai.rag.strategies.summary import (
    SummaryPlan,
    SummaryScopeError,
    SummaryStrategy,
    is_long_summary_request,
)
from private_ai.rag.strategies.vector import VectorStrategy
from private_ai.rag.strategies.web import WebOutcome, WebStrategy

__all__ = [
    "MAX_RESULTS",
    "RRF_K",
    "STRATEGY_TYPES",
    "UNTRUSTED_NOTICE",
    "AutoStrategy",
    "GraphStrategy",
    "HybridStrategy",
    "KeywordStrategy",
    "Strategy",
    "StrategyRegistry",
    "StrategyRetriever",
    "SummaryPlan",
    "SummaryScopeError",
    "SummaryStrategy",
    "VectorStrategy",
    "WebOutcome",
    "WebStrategy",
    "deduplicate",
    "is_long_summary_request",
    "reciprocal_rank_fusion",
    "stamp",
]
