"""Kho vector: Qdrant, một collection cho mỗi dự án.

# Vì sao Qdrant chứ không phải quét tuyến tính trong SQLite

Tầng Rust nạp **toàn bộ** bảng vector lên rồi quét tuyến tính ở mỗi lần hỏi. Với vài
nghìn đoạn thì không sao. Với ``qwen3-embedding:4b`` — 2560 chiều — thì 100.000 đoạn là
một gigabyte đọc từ đĩa cho **mỗi câu hỏi**, và đó là quy mô một thư viện tài liệu công
việc chạm tới sau vài tháng. HNSW của Qdrant đổi phép quét ấy lấy một phép tìm gần đúng
trong vài mili-giây, và giữ nguyên độ chính xác ở mức đủ vì bước xếp hạng lại đằng sau nó
mới là thứ quyết định thứ tự cuối cùng.

# Danh tính của không gian nhúng

Đây là loại hỏng tệ nhất một tầng RAG có thể mắc: cosine giữa hai không gian nhúng khác
nhau **vẫn** trả về một số trong ``[-1, 1]``, vẫn xếp hạng được, vẫn hiện lên giao diện
như một kết quả. Không có gì báo lỗi.

Nên collection mang theo danh tính của thứ đã sinh ra nó — tên model, số chiều, và
:data:`~pai_rag_service.embed.EMBED_INPUT_VERSION` — và :meth:`VectorStore.ensure` **xoá
và dựng lại** khi bất cứ cái nào trong ba lệch đi. Xoá là đúng: vector cũ không sửa được,
và giữ chúng lại chỉ để tránh một lần nhúng lại là đổi một buổi chờ lấy những câu trả lời
sai không giải thích được.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

from qdrant_client import QdrantClient, models

from pai_rag_service.config import VectorConfig
from pai_rag_service.errors import VectorStoreError

__all__ = ["Match", "VectorStore"]

log = logging.getLogger(__name__)

#: Khoá siêu dữ liệu trong payload của một điểm giữ danh tính không gian nhúng. Đặt trên
#: **từng điểm** chứ không chỉ ở cấu hình collection: Qdrant không có chỗ cho siêu dữ liệu
#: cấp collection, và một điểm không nói được nó ra đời từ model nào là một điểm không
#: kiểm được.
FIELD_MODEL = "_embed_model"
FIELD_INPUT = "_embed_input"


@dataclass(slots=True)
class Match:
    """Một đoạn Qdrant trả về."""

    chunk_id: int
    score: float


class VectorStore:
    """Một collection Qdrant cho một dự án."""

    def __init__(self, config: VectorConfig, collection: str) -> None:
        self.collection = collection
        try:
            self.client = QdrantClient(
                url=config.url,
                api_key=config.api_key or None,
                # Một câu hỏi chờ quá mười giây thì người dùng đã bỏ đi rồi; và một
                # Qdrant không chạy phải nói ra nhanh chứ không treo giao diện.
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
        """Bảo đảm collection tồn tại và **hợp** với không gian nhúng đang dùng.

        Trả về ``True`` khi nó vừa được dựng lại — người gọi khi ấy biết mình phải nhúng
        lại toàn bộ thư viện.
        """
        try:
            exists = self.client.collection_exists(self.collection)
        except Exception as err:
            raise VectorStoreError(
                f"không nối được Qdrant: {err}. Dựng nó bằng `docker compose up -d` "
                "trong services/rag/deploy."
            ) from err

        if exists and not self._compatible(dim=dim, model=model, input_version=input_version):
            log.info("không gian nhúng đã đổi — dựng lại collection %s", self.collection)
            self.client.delete_collection(self.collection)
            exists = False

        if exists:
            return False

        self.client.create_collection(
            collection_name=self.collection,
            vectors_config=models.VectorParams(size=dim, distance=models.Distance.COSINE),
        )
        # Lọc theo tài liệu là đường mà `docs.read` và việc xoá đi qua; không có chỉ mục
        # thì Qdrant quét tuần tự payload và phép xoá một tài liệu lớn chạy hàng giây.
        self.client.create_payload_index(
            collection_name=self.collection,
            field_name="document_id",
            field_schema=models.PayloadSchemaType.KEYWORD,
        )
        return True

    def _compatible(self, *, dim: int, model: str, input_version: int) -> bool:
        """Collection hiện có còn dùng được với không gian nhúng này không."""
        try:
            info = self.client.get_collection(self.collection)
            params = info.config.params.vectors
            size = params.size if hasattr(params, "size") else None
            if size is not None and int(size) != dim:
                return False
        except Exception as err:
            log.debug("không đọc được cấu hình collection %s: %s", self.collection, err)
            return False

        # Một collection rỗng hợp với mọi thứ: không có vector nào để lệch.
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
        """Ghi vector. ``chunk_ids`` là khoá — cùng mã thì ghi đè, không sinh bản sao."""
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
        """Trong ``chunk_ids``, những mã **thật sự** đã có vector trong Qdrant.

        Đây là phép hỏi đúng cho câu "còn đoạn nào chưa nhúng". Một phép so số đếm —
        ``count() >= số đoạn`` — nghe rẻ hơn nhưng sai âm thầm ở đúng lúc quan trọng: hai
        kho lệch nhau vì người dùng xoá collection, hoặc vì một lô nhúng hỏng giữa chừng,
        và khi ấy tổng số có thể trùng khớp trong khi từng đoạn thì không.

        ``retrieve`` chỉ trả về điểm có thật, nên tập trả về chính là câu trả lời.
        """
        if not chunk_ids:
            return set()
        found: set[int] = set()
        # Qdrant nhận danh sách mã dài, nhưng một request khổng lồ là một request dễ
        # timeout. 1000 mã một lượt là hàng nghìn đoạn trong vài lần gọi.
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
        """Số vector trong collection.

        **Ném** khi không hỏi được, không trả về 0. Trả 0 cho một Qdrant đang chết là nói
        dối bằng một con số hợp lệ: phía gọi không phân biệt được "chưa nhúng gì" với
        "không hỏi được", và giao diện sẽ báo thư viện trống trong khi nó đầy.
        """
        try:
            if not self.client.collection_exists(self.collection):
                # Chưa nhúng lần nào. Đây là một sự việc, không phải một lỗi.
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
