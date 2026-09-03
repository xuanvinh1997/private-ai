"""Lỗi của tầng RAG, và một luật về cách chúng được viết ra.

Mọi thông điệp ở đây đi thẳng lên giao diện hoặc vào ngữ cảnh của mô hình, nên chúng
phải nói được **người đọc cần làm gì tiếp theo**. "Kết nối thất bại" là một câu không ai
hành động được; "không nối được Qdrant ở 127.0.0.1:6333 — dựng nó bằng `docker compose
up -d` trong services/rag/deploy" thì có.
"""

from __future__ import annotations

__all__ = [
    "ConfigError",
    "EmbedError",
    "ExtractError",
    "GraphError",
    "RagError",
    "RerankError",
    "UnsupportedFile",
    "VectorStoreError",
]


class RagError(Exception):
    """Gốc của mọi lỗi tầng này. Bắt được cái này là bắt được toàn bộ."""


class ConfigError(RagError):
    """Cấu hình thiếu hoặc mâu thuẫn. Người dùng sửa được, ở màn hình Cài đặt."""


class ExtractError(RagError):
    """Một tệp không rút được chữ.

    Luôn kèm đường dẫn: người dùng vừa thả hai mươi tệp vào và cần biết tệp nào.
    """

    def __init__(self, path: str, reason: str) -> None:
        super().__init__(f"{path}: {reason}")
        self.path = path
        self.reason = reason


class UnsupportedFile(ExtractError):
    """Định dạng nằm ngoài tập đọc được. Khác `ExtractError` vì nó **không** đáng thử lại."""


class EmbedError(RagError):
    """Máy chủ nhúng không trả lời, hoặc trả về thứ không dùng được."""


class RerankError(RagError):
    """Reranker hỏng. Luôn bắt được ở tầng trên: xếp hạng lại là bước làm tốt hơn,
    không phải bước bắt buộc, nên nó hỏng thì truy hồi vẫn phải trả về kết quả."""


class VectorStoreError(RagError):
    """Qdrant không với tới được, hoặc collection ở trạng thái không dùng được."""


class GraphError(RagError):
    """Neo4j không với tới được. Cũng luôn bắt được ở tầng trên — chiến lược graph vắng
    mặt thì `auto` phải lùi về `hybrid`, không phải trả lỗi cho người hỏi."""
