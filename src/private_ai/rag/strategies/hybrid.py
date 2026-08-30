"""Dense and lexical retrieval run together, fused by rank."""

from __future__ import annotations

import asyncio
from typing import Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.base import Strategy, deduplicate, reciprocal_rank_fusion, stamp

# How many candidates each arm contributes. Fusing two lists of exactly `limit` items
# gives rank fusion almost nothing to disagree about, so each arm over-fetches.
POOL_FACTOR = 3
MIN_POOL = 10


class HybridStrategy(Strategy):
    name = RetrievalStrategyName.HYBRID.value
    description = (
        "Kết hợp tìm theo ngữ nghĩa và theo từ khóa rồi hợp nhất thứ hạng. Đây là lựa chọn "
        "an toàn khi chưa rõ điều quyết định là cách dùng từ hay là ý nghĩa của câu hỏi."
    )

    async def retrieve(
        self,
        query: str,
        *,
        workspace_id: str,
        limit: int = 5,
        **options: Any,
    ) -> list[Document]:
        text = query.strip()
        if not text:
            return []
        pool = max(MIN_POOL, max(1, limit) * POOL_FACTOR)
        store = self.services.vectors.scoped(workspace_id)
        arms = await asyncio.gather(
            store.asimilarity_search_with_score(text, k=pool),
            store.akeyword_search(text, k=pool),
            # One arm failing — no embedding model, an FTS table that does not exist yet —
            # should cost us that arm, not the whole answer. Both failing still raises.
            return_exceptions=True,
        )
        failures = [arm for arm in arms if isinstance(arm, BaseException)]
        if len(failures) == len(arms):
            raise failures[0]

        rankings: list[list[Document]] = []
        for arm, label in zip(arms, ("vector_score", "keyword_score"), strict=True):
            if isinstance(arm, BaseException):
                continue
            ranked: list[Document] = []
            for document, score in arm:
                document.metadata[label] = float(score)
                ranked.append(document)
            rankings.append(ranked)

        # Reciprocal rank fusion, deliberately. The old implementation summed the raw
        # scores (`max(keyword_score, 0) + max(semantic_score, 0)`) and then dropped any
        # chunk with `keyword_score <= 0 and semantic_score < 0.3`. That summed two
        # incomparable scales — a cosine similarity in [-1, 1] against a lexical overlap
        # count — so whichever arm produced the larger numbers won, and the threshold
        # silently discarded correct dense hits on short queries. Fusing by rank removes
        # both problems and needs no threshold.
        fused = reciprocal_rank_fusion(rankings)
        return stamp(deduplicate(fused, limit), self.name)
