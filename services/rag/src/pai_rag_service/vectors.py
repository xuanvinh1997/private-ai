"""Vector store: Qdrant, one collection per project, replacing a linear scan that costs a
gigabyte of disk per query at scale. Each collection carries the identity of the embedding
space that produced it, and :meth:`VectorStore.ensure` rebuilds when any part of it drifts."""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

from qdrant_client import QdrantClient, models

from pai_rag_service.config import VectorConfig
from pai_rag_service.errors import VectorStoreError

__all__ = ["Match", "VectorStore"]

log = logging.getLogger(__name__)

#: Payload keys holding the embedding-space identity, stored per point because Qdrant has no collection-level metadata.
FIELD_MODEL = "_embed_model"
FIELD_INPUT = "_embed_input"


@dataclass(slots=True)
class Match:
    """One chunk returned by Qdrant."""

    chunk_id: int
    score: float


class VectorStore:
    """One Qdrant collection for one project."""

    def __init__(self, config: VectorConfig, collection: str) -> None:
        self.collection = collection
        try:
            self.client = QdrantClient(
                url=config.url,
                api_key=config.api_key or None,
                # Past ten seconds the user has already left, and a dead Qdrant must say so rather than hang the UI.
                timeout=30,
            )
        except Exception as err:
            raise VectorStoreError(
                f"không dựng được client Qdrant cho {config.url}: {err}"
            ) from err

    def health(self) -> bool:
        try:
            self.client.get_collections()
            return True
        except Exception:
            return False

    def ensure(self, *, dim: int, model: str, input_version: int) -> bool:
        """Ensure the collection exists and matches the embedding space; `True` means it was just rebuilt and the library must be re-embedded."""
        try:
            exists = self.client.collection_exists(self.collection)
        except Exception as err:
            raise VectorStoreError(
                f"không nối được Qdrant: {err}. Dựng nó bằng `docker compose up -d` "
                "trong services/rag/deploy."
            ) from err

        if exists and not self._compatible(dim=dim, model=model, input_version=input_version):
            log.info("embedding space changed - rebuilding collection %s", self.collection)
            self.client.delete_collection(self.collection)
            exists = False

        if exists:
            return False

        self.client.create_collection(
            collection_name=self.collection,
            vectors_config=models.VectorParams(size=dim, distance=models.Distance.COSINE),
        )
        # Filtering by document is the path `docs.read` and deletion take; without an index Qdrant scans payloads sequentially.
        self.client.create_payload_index(
            collection_name=self.collection,
            field_name="document_id",
            field_schema=models.PayloadSchemaType.KEYWORD,
        )
        return True

    def _compatible(self, *, dim: int, model: str, input_version: int) -> bool:
        """Is the existing collection still usable with this embedding space?"""
        try:
            info = self.client.get_collection(self.collection)
            params = info.config.params.vectors
            size = params.size if hasattr(params, "size") else None
            if size is not None and int(size) != dim:
                return False
        except Exception as err:
            log.debug("could not read collection config %s: %s", self.collection, err)
            return False

        # An empty collection matches anything: no vectors to be wrong about.
        sample, _ = self.client.scroll(
            collection_name=self.collection, limit=1, with_payload=True, with_vectors=False
        )
        if not sample:
            return True
        payload = sample[0].payload or {}
        return (
            payload.get(FIELD_MODEL) == model
            and int(payload.get(FIELD_INPUT, -1)) == input_version
        )

    def upsert(
        self,
        *,
        chunk_ids: list[int],
        vectors: list[list[float]],
        payloads: list[dict[str, Any]],
        model: str,
        input_version: int,
    ) -> None:
        """Write vectors. `chunk_ids` is the key - the same id overwrites rather than duplicating."""
        if not chunk_ids:
            return
        if not (len(chunk_ids) == len(vectors) == len(payloads)):
            raise VectorStoreError(
                f"lệch độ dài: {len(chunk_ids)} mã, {len(vectors)} vector, "
                f"{len(payloads)} payload"
            )
        points = [
            models.PointStruct(
                id=chunk_id,
                vector=vector,
                payload={**payload, FIELD_MODEL: model, FIELD_INPUT: input_version},
            )
            for chunk_id, vector, payload in zip(chunk_ids, vectors, payloads, strict=True)
        ]
        try:
            self.client.upsert(collection_name=self.collection, points=points, wait=True)
        except Exception as err:
            raise VectorStoreError(f"không ghi được vector vào Qdrant: {err}") from err

    def search(self, vector: list[float], limit: int) -> list[Match]:
        try:
            found = self.client.query_points(
                collection_name=self.collection,
                query=vector,
                limit=limit,
                with_payload=False,
            )
        except Exception as err:
            raise VectorStoreError(f"Qdrant không trả lời được truy vấn: {err}") from err
        return [Match(chunk_id=int(point.id), score=float(point.score)) for point in found.points]

    def remove_document(self, document_id: str) -> None:
        try:
            self.client.delete(
                collection_name=self.collection,
                points_selector=models.FilterSelector(
                    filter=models.Filter(
                        must=[
                            models.FieldCondition(
                                key="document_id",
                                match=models.MatchValue(value=document_id),
                            )
                        ]
                    )
                ),
                wait=True,
            )
        except Exception as err:
            raise VectorStoreError(f"không xoá được vector của `{document_id}`: {err}") from err

    def existing_ids(self, chunk_ids: list[int]) -> set[int]:
        """Which of `chunk_ids` really have vectors in Qdrant; a count comparison would agree in total while individual chunks were missing."""
        if not chunk_ids:
            return set()
        found: set[int] = set()
        # Qdrant accepts long id lists, but a huge request times out easily; 1000 per call covers thousands of chunks in a few round trips.
        for start in range(0, len(chunk_ids), 1000):
            batch = chunk_ids[start : start + 1000]
            try:
                points = self.client.retrieve(
                    collection_name=self.collection,
                    ids=batch,
                    with_payload=False,
                    with_vectors=False,
                )
            except Exception as err:
                raise VectorStoreError(f"không đọc được mã điểm từ Qdrant: {err}") from err
            found.update(int(point.id) for point in points)
        return found

    def count(self) -> int:
        """Number of vectors in the collection; raises rather than returning 0, since 0 from a dead Qdrant would report an empty library."""
        try:
            if not self.client.collection_exists(self.collection):
                # Never embedded. A fact, not an error.
                return 0
            return int(self.client.count(self.collection, exact=True).count)
        except Exception as err:
            raise VectorStoreError(f"không đếm được vector: {err}") from err

    def drop(self) -> None:
        try:
            if self.client.collection_exists(self.collection):
                self.client.delete_collection(self.collection)
        except Exception as err:
            raise VectorStoreError(f"không xoá được collection: {err}") from err
