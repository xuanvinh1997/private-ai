"""The router: picks a strategy from the shape of the question, without a model call.

Asking a model which retriever to use costs a round trip before retrieval has even
started, and it is not reproducible — the same question can route two ways on two turns,
which makes a bad answer impossible to explain. These rules are cheap, deterministic and
inspectable, and ``explain`` hands the reasoning to the UI verbatim.
"""

from __future__ import annotations

import re
import unicodedata
from typing import TYPE_CHECKING, Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.base import Strategy
from private_ai.rag.strategies.graph import GraphStrategy
from private_ai.rag.strategies.hybrid import HybridStrategy
from private_ai.rag.strategies.keyword import KeywordStrategy
from private_ai.rag.strategies.summary import (
    SummaryPlan,
    SummaryScopeError,
    SummaryStrategy,
    is_long_summary_request,
)
from private_ai.rag.strategies.vector import VectorStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

# Relational phrasing, ASCII-folded so "quan hệ" and "quan he" both hit.
_RELATIONAL_PHRASES = (
    "lien quan",
    "quan he",
    "moi lien he",
    "lien ket",
    "ket noi",
    "ai la",
    "ai da",
    "vai tro cua",
    "anh huong den",
    "phu thuoc vao",
    "relationship",
    "related to",
    "connected to",
    "connection between",
    "who is",
    "how does",
    "depends on",
)
_BETWEEN = re.compile(r"\bgiua\b.*\bva\b|\bbetween\b.*\band\b")

_WORD = re.compile(r"[^\W\d_]+", re.UNICODE)
_QUOTED = re.compile(r"[\"“”'‘’«»]([^\"“”'‘’«»]{2,})[\"“”'‘’«»]")
_CODE_LIKE = re.compile(
    r"\b(?:"
    r"[A-Za-z]{1,12}[-_/]?\d{2,}[\w-]*"  # ND-15, ISO9001, RFC7231
    r"|[A-Z]{2,}(?:[-_]?\d+)?"  # ALLCAPS acronyms and codes
    r"|[A-Za-z]\w*\.[A-Za-z]\w{0,5}"  # bao_cao.pdf, module.function
    r"|\d{4,}"  # long numeric identifiers
    r"|[0-9a-fA-F]{8,}"  # hashes and uuid fragments
    r")\b"
)

MIN_PROPER_NOUNS = 2


def _fold(value: str) -> str:
    decomposed = unicodedata.normalize("NFKD", value.casefold())
    ascii_text = "".join(char for char in decomposed if not unicodedata.combining(char))
    return re.sub(r"[^a-z0-9]+", " ", ascii_text).strip()


def _proper_nouns(query: str) -> list[str]:
    """Capitalised words that are not the first word and not an acronym.

    Two of them in one question usually means the question is about how those two things
    stand to each other, which is what the graph is for.
    """
    tokens = _WORD.findall(query)
    return [
        token
        for position, token in enumerate(tokens)
        if position > 0 and token[:1].isupper() and not token.isupper()
    ]


def _is_relational(query: str) -> bool:
    folded = f" {_fold(query)} "
    if any(f" {phrase} " in folded for phrase in _RELATIONAL_PHRASES):
        return True
    if _BETWEEN.search(folded):
        return True
    return len(set(_proper_nouns(query))) >= MIN_PROPER_NOUNS


def _lexical_signal(query: str) -> str:
    quoted = _QUOTED.search(query)
    if quoted:
        return f'cụm từ trong ngoặc kép "{quoted.group(1)[:40]}"'
    code = _CODE_LIKE.search(query)
    if code:
        return f"mã/định danh {code.group(0)[:40]!r}"
    return ""


class AutoStrategy(Strategy):
    name = RetrievalStrategyName.AUTO.value
    description = (
        "Tự động chọn chiến lược truy hồi theo dạng câu hỏi: tóm tắt toàn bộ tài liệu, "
        "hỏi về quan hệ giữa các thực thể, khớp đúng mã/từ khóa, hay còn lại là kết hợp "
        "ngữ nghĩa và từ khóa. Đây là lựa chọn mặc định khi không có lý do rõ ràng để chỉ "
        "định một chiến lược cụ thể."
    )

    def __init__(self, services: AppServices) -> None:
        super().__init__(services)
        # Strategies are stateless wrappers over the shared stores, so owning a private
        # set here costs nothing and keeps the router free of a dependency on the
        # registry that constructs it.
        self.summary = SummaryStrategy(services)
        self.graph = GraphStrategy(services)
        self.keyword = KeywordStrategy(services)
        self.hybrid = HybridStrategy(services)
        self.vector = VectorStrategy(services)

    def classify(self, query: str) -> tuple[str, str]:
        """Text-only routing: the same cascade minus the database lookup for summaries."""
        if is_long_summary_request(query):
            return self.summary.name, "câu hỏi yêu cầu tóm tắt toàn bộ một tài liệu"
        return self._non_summary(query)

    def explain(self, query: str) -> str:
        name, reason = self.classify(query)
        return f"auto → {name}: {reason}."

    async def choose(
        self,
        query: str,
        *,
        workspace_id: str,
    ) -> tuple[Strategy, str, SummaryPlan | None]:
        """The real cascade. Only the summary branch needs to touch the database."""
        note = ""
        if is_long_summary_request(query):
            try:
                plan = await self.summary.scope(query, workspace_id)
            except SummaryScopeError as exc:
                # The document was found but the requested part of it does not exist.
                # Falling through beats failing the turn, as long as we say why.
                note = f"phạm vi tóm tắt không hợp lệ ({exc})"
            else:
                if plan is not None:
                    return self.summary, f"yêu cầu tóm tắt toàn bộ [{plan.source_label}]", plan
                note = "không xác định được tài liệu cần tóm tắt"
        name, reason = self._non_summary(query)
        if note:
            reason = f"{note}; {reason}"
        chosen = {
            self.graph.name: self.graph,
            self.keyword.name: self.keyword,
            self.hybrid.name: self.hybrid,
            self.vector.name: self.vector,
        }[name]
        return chosen, reason, None

    def _non_summary(self, query: str) -> tuple[str, str]:
        if _is_relational(query):
            return self.graph.name, "câu hỏi hỏi về quan hệ giữa các thực thể"
        signal = _lexical_signal(query)
        if signal:
            return self.keyword.name, f"câu hỏi chứa {signal} cần khớp đúng chữ"
        return self.hybrid.name, "không rõ điều quyết định là từ ngữ hay ý nghĩa"

    async def retrieve(
        self,
        query: str,
        *,
        workspace_id: str,
        limit: int = 5,
        **options: Any,
    ) -> list[Document]:
        if not query.strip():
            return []
        strategy, reason, plan = await self.choose(query, workspace_id=workspace_id)
        if plan is not None:
            documents = self.summary.documents(plan)
        else:
            documents = await strategy.retrieve(
                query,
                workspace_id=workspace_id,
                limit=limit,
                **options,
            )
        for document in documents:
            # `strategy` stays the concrete one so a citation still says how it was found;
            # `routed_by` is what tells the UI a router made that choice.
            document.metadata["strategy"] = strategy.name
            document.metadata["routed_by"] = self.name
            document.metadata["routing_reason"] = reason
        return documents
