from __future__ import annotations

import asyncio
import logging
import math
import re
from collections.abc import Callable
from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path
from time import monotonic
from typing import Any

import numpy as np
from lightrag import LightRAG, QueryParam
from lightrag.kg.shared_storage import initialize_pipeline_status
from lightrag.utils import EmbeddingFunc

from private_ai_api.database import Database
from private_ai_api.schemas import ChatMessage, ChatRequest
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRouter
from private_ai_api.services.rag_anything import (
    RagAnythingOrchestrator,
    RagAnythingUnavailable,
)

logger = logging.getLogger(__name__)

WORKSPACE_SAFE = re.compile(r"[^A-Za-z0-9_-]+")
PROBE_TEXT = "private ai"
ProgressCallback = Callable[[dict[str, object]], None]


@dataclass(slots=True)
class _IndexTracker:
    callback: ProgressCallback
    estimated_chunks: int
    started_at: float
    embedded_vectors: int = 0
    graph_calls: int = 0
    progress: float = 0.45

    def emit(self, step: str, progress: float, detail: str) -> None:
        self.progress = max(self.progress, progress)
        elapsed = max(monotonic() - self.started_at, 0.001)
        self.callback(
            {
                "step": step,
                "progress": min(self.progress, 0.97),
                "detail": detail,
                "embedded_vectors": self.embedded_vectors,
                "estimated_chunks": self.estimated_chunks,
                "vectors_per_second": self.embedded_vectors / elapsed,
                "elapsed_seconds": elapsed,
            }
        )

    def embedded(self, count: int) -> None:
        self.embedded_vectors += count
        ratio = min(self.embedded_vectors / max(self.estimated_chunks, 1), 1.0)
        self.emit(
            "embedding",
            0.5 + ratio * 0.24,
            f"Đã tạo {self.embedded_vectors} vector embedding",
        )

    def graph_started(self) -> None:
        self.graph_calls += 1
        self.emit(
            "graph",
            min(0.78 + self.graph_calls * 0.015, 0.94),
            f"Đang trích xuất thực thể và quan hệ · lượt {self.graph_calls}",
        )


_ACTIVE_INDEX: ContextVar[_IndexTracker | None] = ContextVar(
    "private_ai_active_index",
    default=None,
)
_ACTIVE_GRAPH_MODEL: ContextVar[str] = ContextVar(
    "private_ai_active_graph_model",
    default="",
)


def default_model(database: Database, task: str, fallback: str = "") -> str:
    """The model the user picked for a task, which LightRAG needs at call time."""
    row = database.fetch_one(
        "SELECT model_name FROM model_defaults WHERE task = ?",
        (task,),
    )
    return str(row["model_name"]) if row else fallback


def _make_embed(ai: ProviderRouter, model: str) -> Callable[[list[str]], Any]:
    """Build the callable as a closure over the provider only.

    LightRAG deep-copies the functions it is handed. A bound method would drag the whole
    store along with it, including the LightRAG instance the store caches, and copying a
    half-built instance crashes inside LightRAG itself.
    """

    async def embed(texts: list[str]) -> np.ndarray:
        vectors = await ai.embed(model, list(texts))
        tracker = _ACTIVE_INDEX.get()
        if tracker is not None:
            tracker.embedded(len(texts))
        return np.array(vectors, dtype=np.float32)

    return embed


def _make_complete(ai: ProviderRouter, resolve_model: Callable[[], str]) -> Callable[..., Any]:
    async def complete(
        prompt: str,
        system_prompt: str | None = None,
        history_messages: list[dict[str, Any]] | None = None,
        **_: Any,
    ) -> str:
        tracker = _ACTIVE_INDEX.get()
        if tracker is not None:
            tracker.graph_started()
        messages = [
            *([ChatMessage(role="system", content=system_prompt)] if system_prompt else []),
            *(
                ChatMessage(role=str(item["role"]), content=str(item["content"]))
                for item in (history_messages or [])
            ),
            ChatMessage(role="user", content=prompt),
        ]
        model = _ACTIVE_GRAPH_MODEL.get() or resolve_model()
        result = await ai.chat(
            ChatRequest(model=model, messages=messages, options={"temperature": 0})
        )
        return str(result.get("message", {}).get("content", ""))

    return complete


def _namespace(workspace_id: str) -> str:
    """LightRAG uses the workspace as a storage namespace, so keep it filesystem-safe."""
    return WORKSPACE_SAFE.sub("-", workspace_id).strip("-") or "default"


class LightRagStore:
    """Knowledge graph and vector index for documents, held in files next to the database.

    LightRAG owns chunking, embedding, entity extraction and retrieval. It runs in this
    process against file-backed storages, so nothing here needs a database server. Every
    workspace gets its own LightRAG namespace, which is what keeps one workspace's documents
    out of another's answers.
    """

    def __init__(
        self,
        data_dir: Path,
        ai: ProviderRouter,
        *,
        embedding_model: str,
        resolve_chat_model: Callable[[], str],
        resolve_graph_model: Callable[[], str] | None = None,
        enabled: bool = True,
        top_k: int = 40,
        chunk_top_k: int = 20,
        embedding_batch_size: int = 32,
        embedding_concurrency: int = 4,
    ) -> None:
        self.working_dir = data_dir / "lightrag"
        self.ai = ai
        self.embedding_model = embedding_model
        # Keep resolve_chat_model for callers created before Graph RAG had its own model.
        self.resolve_graph_model = resolve_graph_model or resolve_chat_model
        self.enabled = enabled
        self.top_k = top_k
        self.chunk_top_k = chunk_top_k
        self.embedding_batch_size = embedding_batch_size
        self.embedding_concurrency = embedding_concurrency
        self._instances: dict[str, LightRAG] = {}
        self._dimension: int | None = None
        self._lock = asyncio.Lock()
        self.rag_anything = RagAnythingOrchestrator(self.working_dir / "rag-anything")

    async def _embedding_dimension(self) -> int:
        if self._dimension is None:
            probe = await self.ai.embed(self.embedding_model, [PROBE_TEXT])
            self._dimension = len(probe[0])
        return self._dimension

    async def _instance(self, workspace_id: str) -> LightRAG | None:
        """Build the workspace index on first use, or report it unavailable."""
        if not self.enabled:
            return None
        namespace = _namespace(workspace_id)
        async with self._lock:
            existing = self._instances.get(namespace)
            if existing is not None:
                return existing
            try:
                dimension = await self._embedding_dimension()
                self.working_dir.mkdir(parents=True, exist_ok=True)
                instance = LightRAG(
                    working_dir=str(self.working_dir),
                    workspace=namespace,
                    embedding_func=EmbeddingFunc(
                        embedding_dim=dimension,
                        func=_make_embed(self.ai, self.embedding_model),
                        model_name=self.embedding_model,
                    ),
                    llm_model_func=_make_complete(self.ai, self.resolve_graph_model),
                    llm_model_name=self.resolve_graph_model(),
                    top_k=self.top_k,
                    chunk_top_k=self.chunk_top_k,
                    embedding_batch_num=self.embedding_batch_size,
                    embedding_func_max_async=self.embedding_concurrency,
                )
                await instance.initialize_storages()
                await initialize_pipeline_status()
            except (ProviderUnavailable, OSError, ValueError, IndexError, RuntimeError) as exc:
                logger.warning("LightRAG is unavailable for %s: %s", workspace_id, exc)
                return None
            self._instances[namespace] = instance
            return instance

    async def use_embedding_model(self, name: str) -> None:
        """Swap the embedding model, discarding indexes built at the old vector width."""
        if name == self.embedding_model:
            return
        await self.close()
        async with self._lock:
            self.embedding_model = name
            self._dimension = None

    async def configure_indexing(self, *, batch_size: int, concurrency: int) -> None:
        """Apply UI-managed embedding limits to newly created LightRAG instances."""
        if batch_size == self.embedding_batch_size and concurrency == self.embedding_concurrency:
            return
        await self.close()
        async with self._lock:
            self.embedding_batch_size = batch_size
            self.embedding_concurrency = concurrency

    async def health(self) -> bool:
        return bool(self.enabled and self._instances)

    async def index_document(
        self,
        workspace_id: str,
        document_id: str,
        filename: str,
        text: str,
        on_progress: ProgressCallback | None = None,
        graph_model: str = "",
    ) -> bool:
        if not text.strip():
            return False
        instance = await self._instance(workspace_id)
        if instance is None:
            return False
        tracker = _IndexTracker(
            callback=on_progress or (lambda _event: None),
            estimated_chunks=max(1, math.ceil(len(text) / 3000)),
            started_at=monotonic(),
        )
        tracker.emit("chunking", 0.45, "Đang chia nội dung thành các đoạn có thể tìm kiếm")
        token = _ACTIVE_INDEX.set(tracker)
        model_token = _ACTIVE_GRAPH_MODEL.set(graph_model.strip())
        try:
            try:
                await self.rag_anything.index_text(
                    namespace=_namespace(workspace_id),
                    lightrag=instance,
                    document_id=document_id,
                    filename=filename,
                    text=text,
                    on_progress=tracker.callback,
                )
            except RagAnythingUnavailable:
                await instance.ainsert(text, ids=document_id, file_paths=filename)
        except Exception as exc:  # LightRAG surfaces provider and storage errors alike
            logger.warning("Could not index %s: %s", document_id, exc)
            return False
        finally:
            _ACTIVE_GRAPH_MODEL.reset(model_token)
            _ACTIVE_INDEX.reset(token)
        tracker.emit("finalizing", 0.97, "Đang lưu chỉ mục và hoàn tất")
        return True

    async def delete_document(self, workspace_id: str, document_id: str) -> bool:
        instance = await self._instance(workspace_id)
        if instance is None:
            return False
        try:
            await instance.adelete_by_doc_id(document_id)
        except Exception as exc:
            logger.warning("Could not drop %s from the index: %s", document_id, exc)
            return False
        return True

    async def search(
        self,
        query: str,
        workspace_id: str,
        limit: int = 5,
        *,
        mode: str = "mix",
    ) -> list[dict[str, object]]:
        """Retrieve chunks without asking the model to write an answer."""
        instance = await self._instance(workspace_id)
        if instance is None or not query.strip():
            return []
        try:
            result = await instance.aquery_data(
                query,
                QueryParam(mode=mode, chunk_top_k=max(1, min(limit, 20))),
            )
        except Exception as exc:
            logger.warning("Retrieval failed in %s: %s", workspace_id, exc)
            return []
        chunks = (result.get("data") or {}).get("chunks") or []
        return [
            {
                "content": str(chunk.get("content", "")),
                "filename": str(chunk.get("file_path") or "unknown"),
                "chunk_id": str(chunk.get("chunk_id") or ""),
            }
            for chunk in chunks[: max(1, min(limit, 20))]
            if str(chunk.get("content", "")).strip()
        ]

    async def find_entities(
        self,
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, object]]:
        instance = await self._instance(workspace_id)
        if instance is None:
            return []
        try:
            labels = await instance.get_graph_labels()
        except Exception as exc:
            logger.warning("Could not read entity labels in %s: %s", workspace_id, exc)
            return []
        needle = query.strip().casefold()
        matched = [label for label in labels if not needle or needle in str(label).casefold()]
        return [{"name": str(label)} for label in matched[: max(1, min(limit, 100))]]

    async def knowledge_graph(
        self,
        workspace_id: str,
        entity: str = "*",
        depth: int = 2,
        limit: int = 200,
    ) -> dict[str, object]:
        """Read a slice of the graph: one entity's neighbourhood, or `*` for the whole space."""
        label = entity.strip() or "*"
        empty: dict[str, object] = {"entity": label, "nodes": [], "edges": [], "truncated": False}
        instance = await self._instance(workspace_id)
        if instance is None:
            return empty
        try:
            graph = await instance.get_knowledge_graph(
                label,
                max_depth=max(1, min(depth, 5)),
                max_nodes=max(1, min(limit, 500)),
            )
        except Exception as exc:
            logger.warning("Could not read the graph of %s: %s", workspace_id, exc)
            return empty
        return {
            "entity": label,
            "nodes": [
                {"id": node.id, "labels": list(node.labels), "properties": node.properties}
                for node in graph.nodes
            ],
            "edges": [
                {
                    "source": edge.source,
                    "target": edge.target,
                    "type": edge.type,
                    "properties": edge.properties,
                }
                for edge in graph.edges
            ],
            "truncated": bool(graph.is_truncated),
        }

    async def neighborhood(
        self,
        entity: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        # The MCP tool answers a language model, so edges stay at source/target/type.
        graph = await self.knowledge_graph(workspace_id, entity, depth=2, limit=limit)
        edges = [
            {"source": edge["source"], "target": edge["target"], "type": edge["type"]}
            for edge in graph["edges"]  # type: ignore[union-attr]
        ]
        return {"entity": entity, "nodes": graph["nodes"], "edges": edges}

    async def close(self) -> None:
        instances = list(self._instances.values())
        self._instances.clear()
        self.rag_anything.clear()
        for instance in instances:
            try:
                await instance.finalize_storages()
            except Exception as exc:
                logger.warning("Could not close a LightRAG index cleanly: %s", exc)
