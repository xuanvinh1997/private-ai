"""Merge two ranked lists with Reciprocal Rank Fusion.
RRF looks only at rank, so BM25 and cosine, which share no scale, need no implicit weight.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

__all__ = ["MatchedBy", "RRF_K", "Ranked", "fuse"]

#: RRF constant from Cormack et al. Not a tuning parameter: it flattens the gap between rank 1 and rank 2.
RRF_K = 60.0


class MatchedBy(StrEnum):
    """Why a chunk showed up in the results."""

    KEYWORD = "keyword"
    SEMANTIC = "semantic"
    BOTH = "both"


@dataclass(slots=True)
class Ranked:
    chunk_id: int
    score: float
    matched_by: MatchedBy


def fuse(keyword: list[int], semantic: list[int], limit: int) -> list[Ranked]:
    """Merge two already-ranked lists (best first) into one."""
    merged: list[Ranked] = []
    seen: dict[int, int] = {}

    def contribute(ids: list[int], source: MatchedBy) -> None:
        for index, chunk_id in enumerate(ids):
            # Ranks count from 1: rank 0 would make the denominator `k` and erase the gap to second place.
            contribution = 1.0 / (RRF_K + index + 1)
            at = seen.get(chunk_id)
            if at is None:
                seen[chunk_id] = len(merged)
                merged.append(Ranked(chunk_id, contribution, source))
                continue
            row = merged[at]
            row.score += contribution
            if row.matched_by is not source:
                row.matched_by = MatchedBy.BOTH

    contribute(keyword, MatchedBy.KEYWORD)
    contribute(semantic, MatchedBy.SEMANTIC)

    # Score descending, ties by ascending id, so identical questions always give an identical order.
    merged.sort(key=lambda row: (-row.score, row.chunk_id))
    return merged[:limit]
