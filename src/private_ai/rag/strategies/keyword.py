"""Lexical retrieval: the words the user typed, as they typed them."""

from __future__ import annotations

from typing import Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.base import Strategy, deduplicate, stamp


class KeywordStrategy(Strategy):
    name = RetrievalStrategyName.KEYWORD.value
    description = (
        "Tìm theo từ khóa. Phù hợp nhất khi câu hỏi chứa tên riêng, mã số, số hiệu văn bản, "
        "tên hàm/biến, hoặc một cụm từ đặt trong ngoặc kép cần khớp đúng chữ. Không phù hợp "
        "khi người hỏi diễn đạt lại ý bằng từ ngữ của mình."
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
        store = self.services.vectors.scoped(workspace_id)
        pairs = await store.akeyword_search(text, k=max(1, limit))
        documents: list[Document] = []
        for document, score in pairs:
            document.metadata["score"] = float(score)
            documents.append(document)
        return stamp(deduplicate(documents, limit), self.name)
