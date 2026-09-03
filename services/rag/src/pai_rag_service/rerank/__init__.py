"""Xếp hạng lại, và một luật: **bước này được phép hỏng.**

Truy hồi lai ghép trả về ứng viên theo hai tín hiệu rẻ — BM25 và cosine — rồi reranker
đọc **cặp (câu hỏi, đoạn)** cùng lúc và cho điểm. Đó là lý do nó tốt hơn: cosine so hai
vector đã nén tách rời nhau, còn cross-encoder nhìn thấy cả hai vế cùng một lượt.

Nhưng nó là bước **làm tốt hơn**, không phải bước bắt buộc. Model chưa tải xong, ONNX
Runtime không khởi tạo được, endpoint HTTP không trả lời — không cái nào được phép biến
một lần tìm thành một lần hỏng. :func:`rerank` vì thế nuốt mọi lỗi và trả lại đúng thứ tự
nó nhận vào, kèm một dòng log.

# Về model mặc định

``BAAI/bge-reranker-base`` là **bản ONNX chính thức của BAAI** — repo của họ có sẵn
``onnx/model.onnx``. Nó là XLM-RoBERTa base, đa ngữ thật, đọc được tiếng Việt.

``bge-reranker-v2-m3`` mạnh hơn, nhưng repo chính thức của nó **không** có ONNX; chỉ có
bản export của cộng đồng. Ghim một sản phẩm đóng gói vào repo của người lạ là một rủi ro
nguồn cung không đáng chịu làm mặc định — nên nó là một lựa chọn người dùng tự bật, không
phải thứ có sẵn. Xem ``RerankConfig.model`` và ``RerankConfig.onnx_file``.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Protocol

from pai_rag_service.config import RerankConfig
from pai_rag_service.errors import RerankError

__all__ = ["Reranker", "Scored", "build", "rerank"]

log = logging.getLogger(__name__)


@dataclass(slots=True)
class Scored:
    """Một ứng viên sau khi được chấm lại."""

    index: int
    score: float


class Reranker(Protocol):
    """Seam. Hai bản cài đặt: ONNX trong tiến trình, và một endpoint HTTP."""

    @property
    def id(self) -> str: ...

    def score(self, query: str, passages: list[str]) -> list[float]: ...


def build(config: RerankConfig) -> Reranker | None:
    """Reranker theo cấu hình, hoặc ``None`` khi tắt.

    Ném :class:`RerankError` khi cấu hình sai — người gọi phân biệt được "người dùng tắt
    nó đi" với "người dùng bật nó nhưng khai sai", và hai chuyện đó cần hai câu khác nhau
    trên giao diện.
    """
    if not config.enabled:
        return None
    if config.backend == "http":
        from pai_rag_service.rerank.http import HttpReranker

        if not config.url.strip():
            raise RerankError("backend rerank là `http` nhưng chưa khai `url`")
        return HttpReranker(config)
    from pai_rag_service.rerank.onnx_cross_encoder import OnnxReranker

    return OnnxReranker(config)


def rerank(
    reranker: Reranker | None,
    query: str,
    passages: list[str],
    *,
    top_n: int,
) -> list[Scored]:
    """Chấm lại và cắt còn ``top_n``.

    Không có reranker, hoặc reranker hỏng, thì trả về thứ tự vào — đã cắt. Điểm khi ấy
    giảm dần theo thứ hạng cũ, để phía gọi luôn có một con số để hiển thị mà không phải
    phân biệt hai đường.
    """
    if not passages:
        return []
    if reranker is None:
        return [Scored(index=i, score=1.0 / (i + 1)) for i in range(min(top_n, len(passages)))]

    try:
        scores = reranker.score(query, passages)
        if len(scores) != len(passages):
            raise RerankError(
                f"reranker `{reranker.id}` trả {len(scores)} điểm cho {len(passages)} đoạn"
            )
    except Exception as err:
        # Nuốt có chủ ý — xem docstring của module. Log ở mức warning chứ không debug:
        # một reranker hỏng âm thầm suốt một tuần là một thư viện tệ đi mà không ai biết.
        log.warning("bỏ qua bước xếp hạng lại: %s", err)
        return [Scored(index=i, score=1.0 / (i + 1)) for i in range(min(top_n, len(passages)))]

    ranked = sorted(
        (Scored(index=i, score=float(s)) for i, s in enumerate(scores)),
        key=lambda item: item.score,
        reverse=True,
    )
    return ranked[:top_n]
