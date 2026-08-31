"""What the turn is doing right now, on its way to the chat bubble.

A turn spends most of its wall clock inside two nodes that used to say nothing: retrieval
can map-reduce an entire document through the model, and the first call of a cold model
waits on Ollama to move weights into VRAM. Neither emits a token, so the old UI showed a
motionless "Đang suy nghĩ" for what could be minutes and read as a hang.

Progress rides LangGraph's ``custom`` stream so it arrives interleaved with tokens on the
one channel the UI already consumes, rather than through a second path that would have to
be kept in order with it. ``ProgressSink`` is the protocol the ingestion pipeline and the
model puller already report through — the summary strategy has always accepted one, and
this module is what finally connects the other end.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.protocols import ProgressSink

logger = logging.getLogger("private_ai.agent.progress")

__all__ = ["STAGE_LABELS", "emit", "stage_label", "sink"]

# The stages a turn passes through, in order. The label is what the user reads, so it says
# what is happening to *their* data rather than naming the node that is running.
STAGE_LABELS: dict[str, str] = {
    "plan": "Chọn chiến lược",
    "retrieve": "Tìm trong thư viện",
    "scoping": "Xác định tài liệu",
    "mapping": "Đọc tài liệu",
    "reducing": "Gộp các phần đã đọc",
    "memory": "Đọc bộ nhớ cá nhân",
    "web": "Tìm trên web",
    "prompt": "Chuẩn bị ngữ cảnh",
    "loading": "Nạp mô hình",
    "thinking": "Soạn câu trả lời",
    "tool": "Gọi công cụ",
}


def stage_label(stage: str) -> str:
    """The caption for a stage; an unknown one falls back to its own key, never to blank."""
    key = (stage or "").strip()
    return STAGE_LABELS.get(key, key or "Đang xử lý")


def emit(stage: str, detail: str = "", fraction: float = -1.0) -> None:
    """Report one step of the turn, if there is a run listening.

    Called from inside graph nodes. Outside a LangGraph run — a unit test calling a node
    directly, or a strategy used on its own — there is no writer and this is a no-op, so
    progress reporting can be added anywhere without making the caller check first.
    """
    payload: dict[str, Any] = {
        "kind": "progress",
        "stage": (stage or "").strip(),
        "label": stage_label(stage),
        "detail": (detail or "").strip(),
    }
    if 0.0 <= fraction <= 1.0:
        payload["fraction"] = round(float(fraction), 4)
    _write(payload)


def notice(message: str) -> None:
    """A degraded sub-service. Streamed as it happens, not collected until the end."""
    text = (message or "").strip()
    if text:
        _write({"kind": "notice", "message": text})


def sink(*, offset: float = 0.0, span: float = 1.0) -> ProgressSink:
    """A ``ProgressSink`` that forwards into the turn's progress stream.

    ``offset`` and ``span`` map a sub-task's own 0–1 range onto its share of the turn, so
    a summary that reports "mapping 0.35" inside a retrieval step does not read as though
    the whole turn were a third done.
    """

    def report(stage: str, progress: float, detail: str = "") -> None:
        fraction = offset + span * max(0.0, min(1.0, float(progress)))
        emit(stage, detail, fraction)

    return report


def _write(payload: dict[str, Any]) -> None:
    try:
        from langgraph.config import get_stream_writer

        get_stream_writer()(payload)
    except RuntimeError:
        # No run context: a node called directly from a test, or a strategy used outside
        # the graph. There is nobody to tell, which is not an error.
        return
    except Exception:  # pragma: no cover - a broken writer must not fail the turn
        logger.debug("Không gửi được tiến độ: %s", payload.get("stage"), exc_info=True)
