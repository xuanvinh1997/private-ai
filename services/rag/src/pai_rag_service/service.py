"""Một chỗ giữ trạng thái sống của service: cấu hình, pipeline, retriever, reranker.

# Vì sao cần một lớp riêng cho việc này

Ba thứ ở đây đắt để dựng và rẻ để giữ: kết nối Qdrant, phiên ONNX của reranker (vài giây
và vài trăm megabyte), và handle SQLite. Dựng lại chúng ở mỗi lời gọi tool nghĩa là mỗi
câu hỏi của người dùng trả giá vài giây; giữ chúng mãi mãi nghĩa là đổi model trong Cài
đặt không có tác dụng cho tới khi tắt ứng dụng.

Lối đi ở giữa: giữ, nhưng **soi cấu hình ở mỗi lần chạm**. Cấu hình đổi thì vứt đúng
những thứ bị ảnh hưởng — đổi model nhúng thì bỏ pipeline, đổi model rerank thì bỏ
reranker — chứ không vứt tất cả.

# Reranker dùng chung cho mọi dự án

Nó không phụ thuộc dự án nào: cùng một tệp ONNX chấm điểm cho mọi thư viện. Dựng một bản
cho cả tiến trình thay vì một bản cho mỗi dự án, vì bản thứ hai tốn thêm vài trăm megabyte
để làm đúng việc bản thứ nhất đang làm.
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
    """Trạng thái sống của tiến trình, khoá theo mã dự án."""

    def __init__(self) -> None:
        self._lock = threading.RLock()
        self._config: RagConfig | None = None
        self._pipelines: dict[str, Pipeline] = {}
        self._reranker: Reranker | None = None
        self._reranker_key: tuple | None = None

    # -- cấu hình ----------------------------------------------------------------------

    def config(self) -> RagConfig:
        """Cấu hình hiện hành, và dọn thứ đã cũ khi nó vừa đổi."""
        fresh = load()
        with self._lock:
            old = self._config
            self._config = fresh
            if old is not None and self._invalidates(old, fresh):
                self._drop_pipelines()
            return fresh

    @staticmethod
    def _invalidates(old: RagConfig, new: RagConfig) -> bool:
        """Cấu hình mới có làm pipeline đang giữ trở nên sai không.

        Chỉ ba nhóm: nơi nhúng, nơi lưu vector, và cách cắt đoạn. Đổi model vision hay
        đổi cấu hình rerank **không** làm pipeline sai — chúng được đọc lại ở mỗi lần
        dùng — nên vứt pipeline vì chúng chỉ là một lần chờ vô cớ.
        """
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
                log.debug("lỗi lúc đóng pipeline: %s", err)
        self._pipelines.clear()

    # -- pipeline theo dự án -----------------------------------------------------------

    def pipeline(self, project_id: str = "") -> Pipeline:
        """Pipeline của một dự án, dựng lần đầu rồi giữ lại."""
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
        """Reranker dùng chung. ``None`` khi tắt hoặc khi không dựng được.

        Không ném: xếp hạng lại là bước làm tốt hơn, và một cấu hình rerank sai không
        được phép làm mọi lần tìm hỏng theo.
        """
        config = self.config().rerank
        key = (config.enabled, config.backend, config.model, config.onnx_file, config.url)
        with self._lock:
            if key == self._reranker_key:
                return self._reranker
            try:
                self._reranker = build(config)
            except Exception as err:
                log.warning("không dựng được reranker: %s", err)
                self._reranker = None
            self._reranker_key = key
            return self._reranker

    def close(self) -> None:
        with self._lock:
            self._drop_pipelines()
