"""Reranking, and one rule: this step is allowed to fail.
:func:`rerank` swallows every error and returns the incoming order, because a cross-encoder only improves results.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Protocol

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["Reranker", "Scored", "build", "rerank"]

log = logging.getLogger(__name__)


@dataclass(slots=True)
class Scored:
    """One candidate after rescoring."""

    index: int
    score: float


class Reranker(Protocol):
    """The seam. Two implementations: in-process ONNX, and an HTTP endpoint."""

    @property
    def id(self) -> str: ...

    def score(self, query: str, passages: list[str]) -> list[float]: ...


def build(config: RerankConfig) -> Reranker | None:
    """The configured reranker, or `None` when disabled; raises on bad config so "off" and "misconfigured" stay distinguishable."""
    if not config.enabled:
        return None
    if config.backend == "http":
        from pai_rag_service.rerank.http import HttpReranker

        if not config.url.strip():
            raise RerankError("backend rerank là `http` nhưng chưa khai `url`")
        return HttpReranker(config)
    from pai_rag_service.rerank.onnx_cross_encoder import OnnxReranker

    return OnnxReranker(config)


def rerank(
    reranker: Reranker | None,
    query: str,
    passages: list[str],
    *,
    top_n: int,
) -> list[Scored]:
    """Rescore and cut to `top_n`; with no reranker, or a broken one, the incoming order is returned with descending placeholder scores."""
    if not passages:
        return []
    if reranker is None:
        return [Scored(index=i, score=1.0 / (i + 1)) for i in range(min(top_n, len(passages)))]

    try:
        scores = reranker.score(query, passages)
        if len(scores) != len(passages):
            raise RerankError(
                f"reranker `{reranker.id}` trả {len(scores)} điểm cho {len(passages)} đoạn"
            )
    except Exception as err:
        # Swallowed on purpose - see the module docstring. Warning, not debug: a reranker failing silently for a week is a library quietly getting worse.
        log.warning("skipping the rerank step: %s", err)
        return [Scored(index=i, score=1.0 / (i + 1)) for i in range(min(top_n, len(passages)))]

    ranked = sorted(
        (Scored(index=i, score=float(s)) for i, s in enumerate(scores)),
        key=lambda item: item.score,
        reverse=True,
    )
    return ranked[:top_n]
