"""Vector embedding, with the asymmetric query/document prefixes most retrieval models
need; omitting them silently degrades ranking rather than raising. `EMBED_INPUT_VERSION`
goes into collection metadata, so changing how input text is built rebuilds the store."""

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

#: Bump when the text handed to the embedder changes - see the module docstring.
EMBED_INPUT_VERSION = 1

#: Batch size cap: both servers limit request body size, and a failed large batch loses all of its work.
MAX_BATCH = 64

#: A batch can take a while: the model loads into VRAM on the first call.
EMBED_TIMEOUT = httpx.Timeout(180.0, connect=10.0)

#: `(model name pattern, query prefix, document prefix)`, matched in order on the lowercased name.
PREFIXES: tuple[tuple[re.Pattern[str], str, str], ...] = (
    # Nomic: the prefixes are required, not optional. The model was trained with them.
    (re.compile(r"nomic-embed"), "search_query: ", "search_document: "),
    # The E5 family and multilingual-e5.
    (re.compile(r"\be5\b|multilingual-e5|-e5-"), "query: ", "passage: "),
    # Qwen3 Embedding: an instruction on the query side, bare passages.
    (
        re.compile(r"qwen3-embedding|qwen3_embedding"),
        "Instruct: Given a search query, retrieve relevant passages that answer it\nQuery: ",
        "",
    ),
    # Older zh/en BGE: instruction on the query side. `bge-m3` needs none, so it is excluded - a prefix makes it worse.
    (
        re.compile(r"\bbge-(?!m3)(?:large|base|small)"),
        "Represent this sentence for searching relevant passages: ",
        "",
    ),
    # Google's EmbeddingGemma: both sides prefixed, documents in a `title: ... | text: ...` frame; `title: none` because chunking already prepended the heading.
    (
        re.compile(r"embeddinggemma|embedding-gemma"),
        "task: search result | query: ",
        "title: none | text: ",
    ),
)


def prefixes_for(model: str) -> tuple[str, str]:
    """`(query prefix, document prefix)` for a model; `("", "")` when unknown, since a wrong prefix hurts more than a missing one."""
    name = model.strip().lower()
    for pattern, query, document in PREFIXES:
        if pattern.search(name):
            return query, document
    return "", ""


class _BaseEmbeddings(Embeddings):
    """Shared half of the two implementations: batching, prefixes, response length checks."""

    def __init__(self, provider: ProviderConfig) -> None:
        if not provider.model.strip():
            raise EmbedError("chưa chọn mô hình nhúng. Chọn một mô hình trong Cài đặt.")
        self.provider = provider
        self.query_prefix, self.document_prefix = prefixes_for(provider.model)

    # -- LangChain interface -----------------------------------------------------------

    def embed_documents(self, texts: list[str]) -> list[list[float]]:
        return asyncio.run(self.aembed_documents(texts))

    def embed_query(self, text: str) -> list[float]:
        return asyncio.run(self.aembed_query(text))

    async def aembed_documents(self, texts: list[str]) -> list[list[float]]:
        return await self._embed([self.document_prefix + text for text in texts])

    async def aembed_query(self, text: str) -> list[float]:
        vectors = await self._embed([self.query_prefix + text])
        return vectors[0]

    # -- per-implementation ------------------------------------------------------------

    def _url(self) -> str:
        raise NotImplementedError

    def _body(self, batch: list[str]) -> dict[str, Any]:
        raise NotImplementedError

    def _read(self, payload: dict[str, Any], expected: int) -> list[list[float]]:
        raise NotImplementedError

    # -- shared path -------------------------------------------------------------------

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
                    # Include the body: a bare `404` cannot distinguish "model not pulled" from "wrong endpoint".
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
            # Vectors are matched to passages by index, so a short response must be an error rather than a silent shift.
            raise EmbedError(
                f"model `{model}`: xin {expected} vector nhưng nhận {len(vectors)}"
            )
        return vectors


class OllamaEmbeddings(_BaseEmbeddings):
    """`POST {root}/api/embed`, body `{"model": ..., "input": [...]}`."""

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
    """`POST {root}/v1/embeddings`. Used for OpenAI, LM Studio and anything speaking that protocol."""

    def _url(self) -> str:
        return f"{self.provider.root()}/v1/embeddings"

    def _body(self, batch: list[str]) -> dict[str, Any]:
        return {"model": self.provider.model, "input": batch}

    def _read(self, payload: dict[str, Any], expected: int) -> list[list[float]]:
        rows = payload.get("data")
        if not isinstance(rows, list):
            raise EmbedError("phản hồi thiếu trường `data`")
        # Reorder by `index` rather than trusting array order: the spec allows out-of-order results, and one shift assigns the wrong vector to a passage.
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
    """The embedder for a provider."""
    if provider.kind == "ollama":
        return OllamaEmbeddings(provider)
    return OpenAiEmbeddings(provider)
