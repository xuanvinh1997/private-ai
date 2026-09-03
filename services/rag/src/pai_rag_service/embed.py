"""Nhúng vector, và cái mà tầng Rust đang làm sai.

# Tiền tố bất đối xứng

Phần lớn bộ nhúng truy hồi hiện đại được huấn luyện **bất đối xứng**: câu hỏi và đoạn văn
đi vào mô hình qua hai tiền tố khác nhau. ``nomic-embed-text`` cần ``search_query:`` với
câu hỏi và ``search_document:`` với đoạn; họ E5 cần ``query:``/``passage:``; ``qwen3-
embedding`` nhận một câu chỉ dẫn ở phía câu hỏi và không cần gì ở phía đoạn.

Bỏ tiền tố đi thì **không có gì báo lỗi**. Cosine vẫn trả về một số đẹp trong ``[-1, 1]``,
kết quả vẫn xếp hạng được, giao diện vẫn vẽ ra. Chỉ là thứ hạng kém đi một cách đều đặn ở
mọi câu hỏi — đúng loại hỏng không ai lần ra được nếu không đo.

Tầng Rust (``crates/pai-rag/src/embed.rs``) không có khái niệm "đang nhúng câu hỏi hay
đang nhúng đoạn", trong khi model nhúng mặc định của nó — ``nomic-embed-text`` — lại là
một trong những model cần tiền tố nhất. :data:`PREFIXES` ở đây là bản vá cho đúng chuyện
đó.

# Khoá phiên bản đầu vào

:data:`EMBED_INPUT_VERSION` đi vào siêu dữ liệu của collection. Đổi tiền tố, hay đổi việc
ghép tiêu đề mục vào trước nội dung, là **đổi ý nghĩa của mọi vector đã lưu** — trộn
vector cũ với vector mới trong cùng một collection cho ra thứ hạng sai mà không có gì báo.
Tăng số này khi đổi, và kho sẽ tự dựng lại.
"""

from __future__ import annotations

import asyncio
import re
from typing import Any

import httpx
from langchain_core.embeddings import Embeddings

from pai_rag_service.config import ProviderConfig
from pai_rag_service.errors import EmbedError

__all__ = [
    "EMBED_INPUT_VERSION",
    "MAX_BATCH",
    "PREFIXES",
    "OllamaEmbeddings",
    "OpenAiEmbeddings",
    "embedder_for",
    "prefixes_for",
]

#: Tăng khi cách dựng văn bản đem nhúng thay đổi — xem docstring của module.
EMBED_INPUT_VERSION = 1

#: Trần số phần tử một lô. Cả hai máy chủ đều có giới hạn kích thước thân request, và một
#: lô lớn hỏng là mất toàn bộ công của lô đó. 64 đoạn ~1400 ký tự là khoảng 90 KB.
MAX_BATCH = 64

#: Một lần nhúng cả lô có thể chạy lâu: mô hình phải nạp vào VRAM ở lần gọi đầu tiên.
EMBED_TIMEOUT = httpx.Timeout(180.0, connect=10.0)

#: ``(mẫu tên model, tiền tố câu hỏi, tiền tố đoạn)``, xét theo thứ tự.
#:
#: Mẫu khớp trên tên đã hạ chữ thường, nên ``nomic-embed-text:v1.5`` và
#: ``text-embedding-nomic-embed-text-v1.5`` của LM Studio cùng rơi vào một hàng.
PREFIXES: tuple[tuple[re.Pattern[str], str, str], ...] = (
    # Nomic: tiền tố là bắt buộc, không phải tuỳ chọn. Model được huấn luyện với chúng.
    (re.compile(r"nomic-embed"), "search_query: ", "search_document: "),
    # Họ E5 và multilingual-e5.
    (re.compile(r"\be5\b|multilingual-e5|-e5-"), "query: ", "passage: "),
    # Qwen3 Embedding: một câu chỉ dẫn ở phía câu hỏi, đoạn để trần.
    (
        re.compile(r"qwen3-embedding|qwen3_embedding"),
        "Instruct: Given a search query, retrieve relevant passages that answer it\nQuery: ",
        "",
    ),
    # BGE bản zh/en đời cũ: chỉ dẫn ở phía câu hỏi. `bge-m3` thì **không** cần tiền tố,
    # nên nó phải được loại trừ ở đây — thêm tiền tố vào m3 làm kết quả kém đi.
    (
        re.compile(r"\bbge-(?!m3)(?:large|base|small)"),
        "Represent this sentence for searching relevant passages: ",
        "",
    ),
    # EmbeddingGemma của Google. Định dạng prompt của nó khác hẳn ba họ trên: cả hai phía
    # đều có tiền tố, và phía tài liệu dùng khuôn `title: … | text: …`. Model card ghi
    # `title: none` khi không có tiêu đề — ta luôn ở trường hợp đó, vì tiêu đề mục đã được
    # ghép vào thân đoạn từ trước bởi `chunking.embedding_text_for`.
    (
        re.compile(r"embeddinggemma|embedding-gemma"),
        "task: search result | query: ",
        "title: none | text: ",
    ),
)


def prefixes_for(model: str) -> tuple[str, str]:
    """``(tiền tố câu hỏi, tiền tố đoạn)`` cho một model.

    ``("", "")`` cho model không cần — ``bge-m3``, ``text-embedding-3-*`` của OpenAI, và
    mọi model chưa biết. Mặc định **không thêm gì** là chiều an toàn: thêm nhầm một tiền
    tố vào model không mong đợi nó thì kết quả kém đi, còn thiếu tiền tố thì chỉ là
    không tận dụng được.
    """
    name = model.strip().lower()
    for pattern, query, document in PREFIXES:
        if pattern.search(name):
            return query, document
    return "", ""


class _BaseEmbeddings(Embeddings):
    """Phần chung của hai bản cài đặt: chia lô, gắn tiền tố, kiểm độ dài trả về."""

    def __init__(self, provider: ProviderConfig) -> None:
        if not provider.model.strip():
            raise EmbedError("chưa chọn mô hình nhúng. Chọn một mô hình trong Cài đặt.")
        self.provider = provider
        self.query_prefix, self.document_prefix = prefixes_for(provider.model)

    # -- giao diện LangChain ----------------------------------------------------------

    def embed_documents(self, texts: list[str]) -> list[list[float]]:
        return asyncio.run(self.aembed_documents(texts))

    def embed_query(self, text: str) -> list[float]:
        return asyncio.run(self.aembed_query(text))

    async def aembed_documents(self, texts: list[str]) -> list[list[float]]:
        return await self._embed([self.document_prefix + text for text in texts])

    async def aembed_query(self, text: str) -> list[float]:
        vectors = await self._embed([self.query_prefix + text])
        return vectors[0]

    # -- phần mỗi bản cài đặt tự lo ---------------------------------------------------

    def _url(self) -> str:
        raise NotImplementedError

    def _body(self, batch: list[str]) -> dict[str, Any]:
        raise NotImplementedError

    def _read(self, payload: dict[str, Any], expected: int) -> list[list[float]]:
        raise NotImplementedError

    # -- đường chạy chung -------------------------------------------------------------

    def _headers(self) -> dict[str, str]:
        headers = {"Content-Type": "application/json"}
        if self.provider.api_key:
            headers["Authorization"] = f"Bearer {self.provider.api_key}"
        return headers

    async def _embed(self, texts: list[str]) -> list[list[float]]:
        if not texts:
            return []
        out: list[list[float]] = []
        url = self._url()
        async with httpx.AsyncClient(timeout=EMBED_TIMEOUT) as client:
            for start in range(0, len(texts), MAX_BATCH):
                batch = texts[start : start + MAX_BATCH]
                try:
                    response = await client.post(
                        url, headers=self._headers(), json=self._body(batch)
                    )
                except httpx.HTTPError as err:
                    raise EmbedError(
                        f"không gọi được máy chủ nhúng ở {url}: {err}. Kiểm tra máy chủ "
                        "có đang chạy không."
                    ) from err
                if response.status_code >= 400:
                    head = response.text.strip().splitlines()
                    detail = head[0][:200] if head else ""
                    # Kèm thân trả về: một `404` trơ trọi không phân biệt được "chưa kéo
                    # model về" với "endpoint sai", mà đó là hai việc phải làm khác nhau.
                    raise EmbedError(
                        f"máy chủ nhúng trả {response.status_code} cho model "
                        f"`{self.provider.model}`: {detail}"
                    )
                out.extend(self._read(response.json(), len(batch)))
        return out

    @staticmethod
    def _rows(rows: list[Any], expected: int, model: str) -> list[list[float]]:
        vectors = [[float(value) for value in row] for row in rows]
        if len(vectors) != expected:
            # Ghép vector với đoạn theo chỉ số, nên một máy chủ trả thiếu một phần tử
            # phải là lỗi chứ không phải một sự lệch âm thầm.
            raise EmbedError(
                f"model `{model}`: xin {expected} vector nhưng nhận {len(vectors)}"
            )
        return vectors


class OllamaEmbeddings(_BaseEmbeddings):
    """``POST {root}/api/embed``, thân ``{"model": …, "input": [...]}``."""

    def _url(self) -> str:
        return f"{self.provider.root()}/api/embed"

    def _body(self, batch: list[str]) -> dict[str, Any]:
        return {"model": self.provider.model, "input": batch}

    def _read(self, payload: dict[str, Any], expected: int) -> list[list[float]]:
        rows = payload.get("embeddings")
        if not isinstance(rows, list):
            raise EmbedError("phản hồi của Ollama thiếu trường `embeddings`")
        return self._rows(rows, expected, self.provider.model)


class OpenAiEmbeddings(_BaseEmbeddings):
    """``POST {root}/v1/embeddings``. Dùng cho OpenAI, LM Studio và mọi máy chủ nói cùng
    giao thức."""

    def _url(self) -> str:
        return f"{self.provider.root()}/v1/embeddings"

    def _body(self, batch: list[str]) -> dict[str, Any]:
        return {"model": self.provider.model, "input": batch}

    def _read(self, payload: dict[str, Any], expected: int) -> list[list[float]]:
        rows = payload.get("data")
        if not isinstance(rows, list):
            raise EmbedError("phản hồi thiếu trường `data`")
        # Sắp lại theo `index` chứ không tin thứ tự trong mảng: spec cho phép trả về
        # không theo thứ tự, và một lần lệch ở đây gán vector của đoạn này cho đoạn kia —
        # một lỗi không bao giờ báo, chỉ làm kết quả tìm kiếm sai một cách khó hiểu.
        ordered = sorted(
            enumerate(rows),
            key=lambda pair: (
                pair[1].get("index", pair[0]) if isinstance(pair[1], dict) else pair[0]
            ),
        )
        return self._rows(
            [row.get("embedding", []) for _, row in ordered], expected, self.provider.model
        )


def embedder_for(provider: ProviderConfig) -> _BaseEmbeddings:
    """Bộ nhúng cho một provider."""
    if provider.kind == "ollama":
        return OllamaEmbeddings(provider)
    return OpenAiEmbeddings(provider)
