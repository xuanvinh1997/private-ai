"""Dense retrieval: cosine similarity over the chunk embeddings."""

from __future__ import annotations

from typing import Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.base import Strategy, deduplicate, stamp


class VectorStrategy(Strategy):
    name = RetrievalStrategyName.VECTOR.value
    description = (
        "Tìm theo ngữ nghĩa (vector). Phù hợp nhất với câu hỏi diễn giải lại ý, hỏi về khái "
        "niệm hoặc chủ đề, khi cách dùng từ của người hỏi khác với cách viết trong tài liệu. "
        "Không phù hợp khi cần khớp đúng một tên riêng, mã số hay cụm từ trong ngoặc kép."
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
        pairs = await store.asimilarity_search_with_score(text, k=max(1, limit))
        documents: list[Document] = []
        for document, score in pairs:
            document.metadata["score"] = float(score)
            documents.append(document)
        return stamp(deduplicate(documents, limit), self.name)
