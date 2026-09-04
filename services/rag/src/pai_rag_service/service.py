"""Holds the service's live state: config, pipelines, retriever, reranker.
Expensive to build and cheap to keep, so config is re-read per touch and only affected pieces are dropped.
"""

from __future__ import annotations

import logging
import threading

from pai_rag_service.config import RagConfig, load
from pai_rag_service.pipeline import Pipeline
from pai_rag_service.rerank import Reranker, build
from pai_rag_service.retrieval import Retriever

__all__ = ["Service"]

log = logging.getLogger(__name__)


class Service:
    """Live process state, keyed by project id."""

    def __init__(self) -> None:
        self._lock = threading.RLock()
        self._config: RagConfig | None = None
        self._pipelines: dict[str, Pipeline] = {}
        self._reranker: Reranker | None = None
        self._reranker_key: tuple | None = None

    # -- configuration -----------------------------------------------------------------

    def config(self) -> RagConfig:
        """The current config, dropping anything it just invalidated."""
        fresh = load()
        with self._lock:
            old = self._config
            self._config = fresh
            if old is not None and self._invalidates(old, fresh):
                self._drop_pipelines()
            return fresh

    @staticmethod
    def _invalidates(old: RagConfig, new: RagConfig) -> bool:
        """Does the new config make the cached pipelines wrong? Only embedding, vector store and chunking count; vision and rerank are re-read per use."""
        return (
            old.embedding != new.embedding
            or old.vectors != new.vectors
            or old.chunk != new.chunk
            or old.data_dir != new.data_dir
        )

    def _drop_pipelines(self) -> None:
        for pipeline in self._pipelines.values():
            try:
                pipeline.close()
            except Exception as err:
                log.debug("error while closing pipeline: %s", err)
        self._pipelines.clear()

    # -- per-project pipeline ------------------------------------------------------------

    def pipeline(self, project_id: str = "") -> Pipeline:
        """A project's pipeline, built on first use and then kept."""
        config = self.config()
        project = config.project(project_id)
        with self._lock:
            found = self._pipelines.get(project.id)
            if found is not None:
                return found
            built = Pipeline(config, project)
            self._pipelines[project.id] = built
            return built

    def retriever(self, project_id: str = "") -> Retriever:
        pipeline = self.pipeline(project_id)
        return Retriever(
            pipeline.config,
            pipeline.store,
            pipeline.vectors,
            embedder=pipeline.embedder,
            reranker=self.reranker(),
        )

    # -- reranker ----------------------------------------------------------------------

    def reranker(self) -> Reranker | None:
        """The shared reranker, `None` when disabled or unbuildable; never raises, since reranking only improves results."""
        config = self.config().rerank
        key = (config.enabled, config.backend, config.model, config.onnx_file, config.url)
        with self._lock:
            if key == self._reranker_key:
                return self._reranker
            try:
                self._reranker = build(config)
            except Exception as err:
                log.warning("could not build reranker: %s", err)
                self._reranker = None
            self._reranker_key = key
            return self._reranker

    def close(self) -> None:
        with self._lock:
            self._drop_pipelines()
