from __future__ import annotations

import asyncio
import logging
import re
from collections.abc import Callable
from pathlib import Path
from typing import Any

import numpy as np
from lightrag import LightRAG, QueryParam
from lightrag.kg.shared_storage import initialize_pipeline_status
from lightrag.utils import EmbeddingFunc

from private_ai_api.database import Database
from private_ai_api.schemas import ChatMessage, ChatRequest
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRouter

logger = logging.getLogger(__name__)

WORKSPACE_SAFE = re.compile(r"[^A-Za-z0-9_-]+")
PROBE_TEXT = "private ai"


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
        return np.array(vectors, dtype=np.float32)

    return embed


def _make_complete(ai: ProviderRouter, resolve_model: Callable[[], str]) -> Callable[..., Any]:
    async def complete(
        prompt: str,
        system_prompt: str | None = None,
        history_messages: list[dict[str, Any]] | None = None,
        **_: Any,
    ) -> str:
        messages = [
            *([ChatMessage(role="system", content=system_prompt)] if system_prompt else []),
            *(
                ChatMessage(role=str(item["role"]), content=str(item["content"]))
                for item in (history_messages or [])
            ),
            ChatMessage(role="user", content=prompt),
        ]
        result = await ai.chat(
            ChatRequest(model=resolve_model(), messages=messages, options={"temperature": 0})
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
        enabled: bool = True,
        top_k: int = 40,
        chunk_top_k: int = 20,
    ) -> None:
        self.working_dir = data_dir / "lightrag"
        self.ai = ai
        self.embedding_model = embedding_model
        self.resolve_chat_model = resolve_chat_model
        self.enabled = enabled
        self.top_k = top_k
        self.chunk_top_k = chunk_top_k
        self._instances: dict[str, LightRAG] = {}
        self._dimension: int | None = None
        self._lock = asyncio.Lock()

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
                    llm_model_func=_make_complete(self.ai, self.resolve_chat_model),
                    llm_model_name=self.resolve_chat_model(),
                    top_k=self.top_k,
                    chunk_top_k=self.chunk_top_k,
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

    async def health(self) -> bool:
        return bool(self.enabled and self._instances)

    async def index_document(
        self,
        workspace_id: str,
        document_id: str,
        filename: str,
        text: str,
    ) -> bool:
        if not text.strip():
            return False
        instance = await self._instance(workspace_id)
        if instance is None:
            return False
        try:
            await instance.ainsert(text, ids=document_id, file_paths=filename)
        except Exception as exc:  # LightRAG surfaces provider and storage errors alike
            logger.warning("Could not index %s: %s", document_id, exc)
            return False
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

    async def neighborhood(
        self,
        entity: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        instance = await self._instance(workspace_id)
        if instance is None:
            return {"entity": entity, "nodes": [], "edges": []}
        try:
            graph = await instance.get_knowledge_graph(
                entity,
                max_depth=2,
                max_nodes=max(1, min(limit, 200)),
            )
        except Exception as exc:
            logger.warning("Could not read the neighbourhood of %s: %s", entity, exc)
            return {"entity": entity, "nodes": [], "edges": []}
        return {
            "entity": entity,
            "nodes": [
                {"id": node.id, "labels": list(node.labels), "properties": node.properties}
                for node in graph.nodes
            ],
            "edges": [
                {"source": edge.source, "target": edge.target, "type": edge.type}
                for edge in graph.edges
            ],
        }

    async def close(self) -> None:
        instances = list(self._instances.values())
        self._instances.clear()
        for instance in instances:
            try:
                await instance.finalize_storages()
            except Exception as exc:
                logger.warning("Could not close a LightRAG index cleanly: %s", exc)
