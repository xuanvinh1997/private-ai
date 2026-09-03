"""Hợp nhất hai bảng xếp hạng bằng Reciprocal Rank Fusion.

# Vì sao là RRF chứ không phải cộng điểm

BM25 của FTS5 và cosine của Qdrant **không cùng thang đo**. BM25 là một số âm không chặn
dưới, phụ thuộc độ dài tài liệu và tần suất từ trong cả kho; cosine nằm gọn trong
``[-1, 1]`` và với hầu hết bộ nhúng hiện đại thì mọi cặp văn bản tiếng Việt bất kỳ đã rơi
vào khoảng ``0.6–0.9``. Cộng thẳng hai con số đó — hay chuẩn hoá rồi cộng — cho ra một
trọng số ngầm mà không ai chọn: tuỳ kho tài liệu, một bên áp đảo bên kia, và nó đổi khi
người dùng nạp thêm tệp.

RRF chỉ nhìn **thứ hạng** nên miễn nhiễm với chuyện đó: ``score = Σ 1/(k + rank)``. Một
đoạn đứng nhất ở một bảng và vắng mặt ở bảng kia được ``1/61``; một đoạn đứng thứ ba ở cả
hai bảng được ``1/63 + 1/63``, và nó thắng — đúng ý: đồng thuận giữa hai cách tìm là bằng
chứng mạnh hơn một lần đứng nhất ở một cách.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

__all__ = ["MatchedBy", "RRF_K", "Ranked", "fuse"]

#: Hằng số của RRF, lấy từ bài gốc của Cormack và cộng sự.
#:
#: Đây **không** phải một tham số để chỉnh. Nó làm phẳng chênh lệch giữa hạng 1 và hạng 2
#: để một bảng xếp hạng tự tin không nuốt trọn kết quả; hạ nó xuống là quay về gần với
#: việc chỉ tin một nhánh.
RRF_K = 60.0


class MatchedBy(StrEnum):
    """Vì sao một đoạn có mặt trong kết quả."""

    KEYWORD = "keyword"
    SEMANTIC = "semantic"
    BOTH = "both"


@dataclass(slots=True)
class Ranked:
    chunk_id: int
    score: float
    matched_by: MatchedBy


def fuse(keyword: list[int], semantic: list[int], limit: int) -> list[Ranked]:
    """Hợp nhất hai danh sách **đã xếp hạng** (tốt nhất trước) thành một."""
    merged: list[Ranked] = []
    seen: dict[int, int] = {}

    def contribute(ids: list[int], source: MatchedBy) -> None:
        for index, chunk_id in enumerate(ids):
            # Hạng đếm từ 1: hạng 0 làm mẫu số bằng `k` cho phần tử đầu tiên của cả hai
            # bảng và xoá mất chênh lệch giữa nó với phần tử thứ hai.
            contribution = 1.0 / (RRF_K + index + 1)
            at = seen.get(chunk_id)
            if at is None:
                seen[chunk_id] = len(merged)
                merged.append(Ranked(chunk_id, contribution, source))
                continue
            row = merged[at]
            row.score += contribution
            if row.matched_by is not source:
                row.matched_by = MatchedBy.BOTH

    contribute(keyword, MatchedBy.KEYWORD)
    contribute(semantic, MatchedBy.SEMANTIC)

    # Xếp theo điểm giảm dần, hoà thì theo mã tăng dần — để cùng một câu hỏi luôn cho ra
    # cùng một thứ tự. Một thứ tự đổi giữa hai lần chạy giống hệt nhau là thứ không gỡ nổi.
    merged.sort(key=lambda row: (-row.score, row.chunk_id))
    return merged[:limit]
