"""Các chiến lược truy hồi, và bộ định tuyến chọn giữa chúng.

# Bốn chiến lược, và khi nào dùng cái nào

``keyword``  — câu hỏi chứa tên riêng, số hiệu văn bản, mã định danh, hoặc một cụm đặt
trong ngoặc kép cần khớp đúng chữ. BM25 trên FTS5, không cần Qdrant.

``vector``   — người hỏi diễn đạt lại ý bằng từ ngữ của mình; câu hỏi về khái niệm. Không
hợp khi cần khớp đúng một tên riêng, vì bộ nhúng làm nhoè chính chỗ đó.

``hybrid``   — mặc định. Chạy cả hai rồi hợp nhất bằng RRF, sau đó xếp lại bằng
cross-encoder. Đây là đường trả lời đúng nhất cho một câu hỏi thông thường.

``auto``     — chọn hộ bằng **luật**, không gọi mô hình.

# Vì sao `auto` định tuyến bằng luật

Hỏi một mô hình nên dùng retriever nào tốn một vòng round-trip trước khi truy hồi bắt
đầu, và cùng một câu hỏi có thể định tuyến hai kiểu ở hai lượt — khiến một câu trả lời sai
trở nên không giải thích được. Luật thì đọc được, kiểm chứng được, và lý do được ghi vào
kết quả (``routed_by``) nên giao diện nói được vì sao.

# Xếp hạng lại nằm ở đâu

Sau hợp nhất, trước khi cắt. Lấy về :attr:`RerankConfig.candidates` ứng viên rồi cross-
encoder chấm lại và giữ ``top_n``. Đây là chỗ chất lượng thật sự đến từ: BM25 và cosine
là hai tín hiệu rẻ dùng để **thu hẹp**, cross-encoder mới là thứ đọc cả câu hỏi lẫn đoạn
cùng một lượt.
"""

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

#: Cụm cho thấy người dùng muốn tóm tắt **cả** tài liệu, không phải tìm một chi tiết.
SUMMARY_HINTS = re.compile(
    r"\b(tóm tắt|tóm lược|tổng hợp|nội dung chính|nói về (cái )?gì|summar(y|ize)|overview)\b",
    re.IGNORECASE,
)
#: Cụm cho thấy câu hỏi về **quan hệ giữa các thực thể** — việc của graph.
RELATION_HINTS = re.compile(
    r"\b(liên quan|liên hệ|quan hệ|ảnh hưởng|dẫn đến|so với|giữa .+ và |ai (là|đã)|"
    r"related to|relationship|connection between)\b",
    re.IGNORECASE,
)
QUOTED = re.compile(r'"[^"]{2,}"|“[^”]{2,}”')

#: Một "từ" theo nghĩa của mã định danh: chữ, số, và những dấu nối hay nằm giữa chúng.
#: Dấu gạch phải nằm **trong** token, không tách nó ra — đó là chỗ bản đầu tiên của hàm
#: này sai: nó đòi chữ và số cùng nằm trong một khối `\w+`, nên `HD-2026-0042` bị xé
#: thành `HD`, `2026`, `0042` và không mảnh nào vừa có chữ vừa có số. Câu hỏi đúng bằng
#: một mã hợp đồng vì thế được định tuyến sang `hybrid` thay vì `keyword`.
TOKEN = re.compile(r"[A-Za-z0-9]+(?:[-_./][A-Za-z0-9]+)*")


def has_identifier(text: str) -> bool:
    """Câu hỏi có chứa mã định danh không — ``HD-2026-0042``, ``NV001``, ``v1.5``.

    Phép thử là **vừa có chữ vừa có số trong cùng một token**. Đây là thứ bộ nhúng làm
    nhoè (mọi mã hợp đồng trông giống nhau trong không gian vector) và BM25 khớp chính
    xác, nên nó là tín hiệu mạnh nhất để chọn nhánh từ khoá.

    Đòi ít nhất ba ký tự để ``A1`` hay ``số 3`` không kéo cả câu hỏi bình thường sang
    nhánh từ khoá.
    """
    for token in TOKEN.findall(text):
        if len(token) < 3:
            continue
        if any(c.isdigit() for c in token) and any(c.isalpha() for c in token):
            return True
    return False


@dataclass(slots=True)
class Hit:
    """Một đoạn trả về cho người hỏi, đủ để trích dẫn kiểm chứng được."""

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
        """In ra cho mô hình đọc.

        Bắt đầu bằng ``[tên tài liệu #đoạn — mục — trang]`` vì mô hình phải **trích dẫn
        được**: người dùng đọc câu trả lời sẽ hỏi "chỗ nào nói thế", và một câu trả lời
        không chỉ ra được đoạn nào của tài liệu nào thì không kiểm chứng được.
        """
        parts = [f"{self.title} #{self.ordinal}"]
        if self.section:
            parts.append(self.section)
        if self.page:
            parts.append(f"trang {self.page}")
        return f"[{' — '.join(parts)}]\n{self.text}"


def route(query: str) -> tuple[str, str]:
    """``(chiến lược, lý do)`` cho một câu hỏi. Thuần luật, không gọi mô hình.

    Thứ tự xét là thứ tự ưu tiên, và nó cố ý: một câu vừa xin tóm tắt vừa có tên riêng
    thì vẫn là một yêu cầu tóm tắt.
    """
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
    """Truy hồi trên thư viện của một dự án."""

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

    # -- chiến lược ---------------------------------------------------------------------

    def keyword(self, query: str, limit: int) -> list[Hit]:
        ids = self.store.search_keyword(query, limit)
        return self._hits(ids, MatchedBy.KEYWORD.value, {})

    async def semantic_ids(self, query: str, limit: int) -> list[int]:
        """Mã đoạn theo cosine, hoặc danh sách rỗng khi phần ngữ nghĩa chưa dùng được.

        **Không** ném: bộ nhúng tắt hay Qdrant chết không được phép biến một lần tìm
        thành một lần hỏng — nhánh từ khoá vẫn trả lời được.
        """
        try:
            vector = await self.embedder.aembed_query(query)
        except EmbedError as err:
            log.warning("bỏ qua phần ngữ nghĩa: %s", err)
            return []
        try:
            return [match.chunk_id for match in self.vectors.search(vector, limit)]
        except VectorStoreError as err:
            log.warning("bỏ qua phần ngữ nghĩa: %s", err)
            return []

    async def vector(self, query: str, limit: int) -> list[Hit]:
        ids = await self.semantic_ids(query, limit)
        return self._hits(ids, MatchedBy.SEMANTIC.value, {})

    async def hybrid(self, query: str, limit: int) -> list[Hit]:
        """BM25 hợp nhất với cosine bằng RRF, rồi xếp lại bằng cross-encoder."""
        # Lấy sâu hơn `limit` ở mỗi nhánh trước khi hợp nhất: một đoạn đứng hạng 15 ở cả
        # hai bảng đáng lên đầu, mà cắt ở `limit` thì nó không bao giờ vào tới phép hợp
        # nhất. Cũng là tập ứng viên cho bước xếp hạng lại.
        pool = max(self.config.rerank.candidates, limit * 4, 20)
        keyword_ids = self.store.search_keyword(query, pool)
        semantic_ids = await self.semantic_ids(query, pool)

        ranked = fuse(keyword_ids, semantic_ids, pool)
        if not ranked:
            return []

        rows = {row.id: row for row in self.store.chunks_by_id([r.chunk_id for r in ranked])}
        # Giữ đúng thứ tự của phép hợp nhất, và bỏ mã không còn hàng nào — vector mồ côi
        # của một tài liệu đã bị rút ngắn rơi ra ở đây.
        ordered = [(r, rows[r.chunk_id]) for r in ranked if r.chunk_id in rows]
        if not ordered:
            return []

        return self._rerank(query, ordered, limit)

    def _rerank(self, query: str, ordered: list[tuple], limit: int) -> list[Hit]:
        """Xếp lại tập ứng viên và cắt còn ``limit``."""
        from pai_rag_service.rerank import rerank

        passages = [row.body for _, row in ordered]
        scored = rerank(self.reranker, query, passages, top_n=limit)
        out: list[Hit] = []
        for item in scored:
            ranked, row = ordered[item.index]
            out.append(self._hit(row, item.score, ranked.matched_by.value))
        return out

    def read(self, document_id: str, offset: int, limit: int) -> list[Hit]:
        """Đọc một tài liệu theo thứ tự, từng đoạn một."""
        rows = self.store.chunks_of(document_id, offset, limit)
        # Điểm `0.0` vì đây là đọc tuần tự, không phải xếp hạng — một điểm bịa ra ở đây sẽ
        # được giao diện vẽ ra như thể nó có nghĩa.
        return [self._hit(row, 0.0, "read") for row in rows]

    # -- dựng kết quả -------------------------------------------------------------------

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
