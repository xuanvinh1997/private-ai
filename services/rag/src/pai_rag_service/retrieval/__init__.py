"""Retrieval strategies (keyword, vector, hybrid) and the router that picks between them.
`auto` routes by rule, not by asking a model: rules are readable, reproducible, and their
reason ships with the result. Reranking sits after fusion and before the cut."""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass

from pai_rag_service.config import RagConfig
from pai_rag_service.errors import EmbedError, VectorStoreError
from pai_rag_service.retrieval.fusion import MatchedBy, fuse
from pai_rag_service.store import ChunkRow, Store
from pai_rag_service.vectors import VectorStore

__all__ = ["Hit", "Retriever", "route"]

log = logging.getLogger(__name__)

#: Phrases showing the user wants a whole-document summary rather than one detail.
SUMMARY_HINTS = re.compile(
    r"\b(tóm tắt|tóm lược|tổng hợp|nội dung chính|nói về (cái )?gì|summar(y|ize)|overview)\b",
    re.IGNORECASE,
)
#: Phrases showing a question about relations between entities - the graph's job.
RELATION_HINTS = re.compile(
    r"\b(liên quan|liên hệ|quan hệ|ảnh hưởng|dẫn đến|so với|giữa .+ và |ai (là|đã)|"
    r"related to|relationship|connection between)\b",
    re.IGNORECASE,
)
QUOTED = re.compile(r'"[^"]{2,}"|“[^”]{2,}”')

#: A "word" in the identifier sense; the separators must stay *inside* the token, or `HD-2026-0042` splits into pieces that each look alphabetic or numeric but never both.
TOKEN = re.compile(r"[A-Za-z0-9]+(?:[-_./][A-Za-z0-9]+)*")


def has_identifier(text: str) -> bool:
    """Does the query contain an identifier? The test is letters and digits in one token of at least three characters - what embedders blur and BM25 matches exactly."""
    for token in TOKEN.findall(text):
        if len(token) < 3:
            continue
        if any(c.isdigit() for c in token) and any(c.isalpha() for c in token):
            return True
    return False


@dataclass(slots=True)
class Hit:
    """One chunk returned to the asker, enough for a verifiable citation."""

    chunk_id: int
    document_id: str
    title: str
    path: str
    ordinal: int
    section: str
    page: int
    text: str
    score: float
    matched_by: str

    def as_dict(self) -> dict[str, object]:
        return {
            "chunkId": self.chunk_id,
            "documentId": self.document_id,
            "title": self.title,
            "path": self.path,
            "ordinal": self.ordinal,
            "section": self.section,
            "page": self.page,
            "text": self.text,
            "score": round(self.score, 4),
            "matchedBy": self.matched_by,
        }

    def render(self) -> str:
        """Render for the model; the `[title #ordinal - section - page]` prefix is what makes the answer citable."""
        parts = [f"{self.title} #{self.ordinal}"]
        if self.section:
            parts.append(self.section)
        if self.page:
            parts.append(f"trang {self.page}")
        return f"[{' — '.join(parts)}]\n{self.text}"


def route(query: str) -> tuple[str, str]:
    """`(strategy, reason)` for a query, rules only; the order is the priority order, so a summary request stays a summary request."""
    text = query.strip()
    if SUMMARY_HINTS.search(text):
        return "summary", "câu hỏi xin tóm tắt cả tài liệu"
    if RELATION_HINTS.search(text):
        return "graph", "câu hỏi về quan hệ giữa các thực thể"
    if QUOTED.search(text):
        return "keyword", "câu hỏi có cụm trong ngoặc kép, cần khớp đúng chữ"
    if has_identifier(text):
        return "keyword", "câu hỏi chứa mã định danh, cần khớp đúng chữ"
    return "hybrid", "câu hỏi thông thường"


class Retriever:
    """Retrieval over one project's library."""

    def __init__(
        self,
        config: RagConfig,
        store: Store,
        vectors: VectorStore,
        embedder=None,
        reranker=None,
    ) -> None:
        self.config = config
        self.store = store
        self.vectors = vectors
        self._embedder = embedder
        self.reranker = reranker

    @property
    def embedder(self):
        if self._embedder is None:
            from pai_rag_service.embed import embedder_for

            self._embedder = embedder_for(self.config.embedding)
        return self._embedder

    # -- strategies ------------------------------------------------------------------------

    def keyword(self, query: str, limit: int) -> list[Hit]:
        ids = self.store.search_keyword(query, limit)
        return self._hits(ids, MatchedBy.KEYWORD.value, {})

    async def semantic_ids(self, query: str, limit: int) -> list[int]:
        """Chunk ids by cosine, or an empty list when the semantic half is unusable; never raises, since the keyword branch can still answer."""
        try:
            vector = await self.embedder.aembed_query(query)
        except EmbedError as err:
            log.warning("skipping the semantic half: %s", err)
            return []
        try:
            return [match.chunk_id for match in self.vectors.search(vector, limit)]
        except VectorStoreError as err:
            log.warning("skipping the semantic half: %s", err)
            return []

    async def vector(self, query: str, limit: int) -> list[Hit]:
        ids = await self.semantic_ids(query, limit)
        return self._hits(ids, MatchedBy.SEMANTIC.value, {})

    async def hybrid(self, query: str, limit: int) -> list[Hit]:
        """BM25 fused with cosine via RRF, then reranked by the cross-encoder."""
        # Take deeper than `limit` on each branch before fusing: a chunk ranked 15th in both deserves the top, and it is also the reranker's candidate pool.
        pool = max(self.config.rerank.candidates, limit * 4, 20)
        keyword_ids = self.store.search_keyword(query, pool)
        semantic_ids = await self.semantic_ids(query, pool)

        ranked = fuse(keyword_ids, semantic_ids, pool)
        if not ranked:
            return []

        rows = {row.id: row for row in self.store.chunks_by_id([r.chunk_id for r in ranked])}
        # Keep the fusion order and drop ids with no row - orphan vectors of a shortened document fall out here.
        ordered = [(r, rows[r.chunk_id]) for r in ranked if r.chunk_id in rows]
        if not ordered:
            return []

        return self._rerank(query, ordered, limit)

    def _rerank(self, query: str, ordered: list[tuple], limit: int) -> list[Hit]:
        """Rescore the candidate pool and cut to `limit`."""
        from pai_rag_service.rerank import rerank

        passages = [row.body for _, row in ordered]
        scored = rerank(self.reranker, query, passages, top_n=limit)
        out: list[Hit] = []
        for item in scored:
            ranked, row = ordered[item.index]
            out.append(self._hit(row, item.score, ranked.matched_by.value))
        return out

    def read(self, document_id: str, offset: int, limit: int) -> list[Hit]:
        """Read one document in order, chunk by chunk."""
        rows = self.store.chunks_of(document_id, offset, limit)
        # Score `0.0` because this is sequential reading, not ranking; an invented score would be drawn as if it meant something.
        return [self._hit(row, 0.0, "read") for row in rows]

    # -- building results ------------------------------------------------------------------

    def _hits(self, ids: list[int], matched_by: str, scores: dict[int, float]) -> list[Hit]:
        rows = {row.id: row for row in self.store.chunks_by_id(ids)}
        out: list[Hit] = []
        for rank, chunk_id in enumerate(ids):
            row = rows.get(chunk_id)
            if row is None:
                continue
            out.append(self._hit(row, scores.get(chunk_id, 1.0 / (rank + 1)), matched_by))
        return out

    @staticmethod
    def _hit(row: ChunkRow, score: float, matched_by: str) -> Hit:
        return Hit(
            chunk_id=row.id,
            document_id=row.document_id,
            title=row.title,
            path=row.path,
            ordinal=row.ordinal,
            section=row.section,
            page=row.page,
            text=row.body,
            score=score,
            matched_by=matched_by,
        )
