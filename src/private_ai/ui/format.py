"""Display formatting, ported from ``apps/web/src/format.ts``.

Same rounding rules and the same Vietnamese strings, so a number the user saw in the old
build reads identically in this one.
"""

from __future__ import annotations

from datetime import UTC, datetime

__all__ = [
    "elide",
    "format_bytes",
    "format_count",
    "format_file_size",
    "format_percent",
    "format_relative_time",
    "initials_of",
    "short_model_name",
    "stage_label",
    "status_label",
]

_GIB = 1024**3

# Ingestion stages from the ProgressSink contract.
STAGE_LABELS: dict[str, str] = {
    "queued": "Đang chờ xử lý",
    "extracting": "Đang trích xuất nội dung",
    "normalizing": "Đang chuẩn hóa văn bản",
    "chunking": "Đang chia đoạn",
    "embedding": "Đang tạo embedding",
    "indexing": "Đang lập chỉ mục",
    "completed": "Hoàn tất",
    "failed": "Xử lý lỗi",
}

STATUS_LABELS: dict[str, str] = {
    "pending": "Đang chờ",
    "queued": "Đang chờ",
    "processing": "Đang xử lý",
    "ready": "Sẵn sàng",
    "failed": "Lỗi",
    "needs_ocr": "Cần đọc bằng OCR",
    "deleted": "Đã xóa",
}


def format_bytes(num_bytes: float | int | None) -> str:
    """Always GB — this is the VRAM/model-size formatter, and mixing units in a list of
    models makes them impossible to compare at a glance."""
    if not num_bytes:
        return "0 GB"
    gib = float(num_bytes) / _GIB
    return f"{gib:.1f} GB" if gib < 10 else f"{gib:.0f} GB"


def format_file_size(num_bytes: float | int | None) -> str:
    """Scales to the file's own size, so a small note is not reported as "0.0 MB"."""
    value = float(num_bytes or 0)
    if value < 1024:
        return f"{int(value)} B"
    units = ("KB", "MB", "GB")
    value /= 1024
    unit = 0
    while value >= 1024 and unit < len(units) - 1:
        value /= 1024
        unit += 1
    return f"{value:.1f} {units[unit]}" if value < 10 else f"{round(value)} {units[unit]}"


def _parse(value: str | datetime | None) -> datetime | None:
    if isinstance(value, datetime):
        return value if value.tzinfo else value.replace(tzinfo=UTC)
    text = (value or "").strip()
    if not text:
        return None
    if text.endswith("Z"):
        text = f"{text[:-1]}+00:00"
    try:
        parsed = datetime.fromisoformat(text)
    except ValueError:
        return None
    # SQLite writes naive UTC timestamps; treating them as local would make everything
    # look hours old the moment the user is not in UTC.
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=UTC)


def format_relative_time(value: str | datetime | None) -> str:
    parsed = _parse(value)
    if parsed is None:
        return ""
    elapsed = (datetime.now(UTC) - parsed).total_seconds()
    minutes = max(0, int(elapsed // 60))
    if minutes < 1:
        return "Bây giờ"
    if minutes < 60:
        return f"{minutes} phút"
    hours = minutes // 60
    if hours < 24:
        return f"{hours} giờ"
    local = parsed.astimezone()
    return f"{local.day:02d}/{local.month:02d}"


def format_percent(fraction: float | None) -> str:
    return f"{round(max(0.0, min(1.0, float(fraction or 0.0))) * 100)}%"


def format_count(value: int | None) -> str:
    """vi-VN groups thousands with a dot."""
    return f"{int(value or 0):,}".replace(",", ".")


def stage_label(stage: str) -> str:
    return STAGE_LABELS.get((stage or "").strip(), stage or "")


def status_label(status: str) -> str:
    return STATUS_LABELS.get((status or "").strip(), status or "")


def elide(text: str, limit: int = 60) -> str:
    value = (text or "").strip()
    return value if len(value) <= limit else f"{value[: max(1, limit - 1)]}…"


def short_model_name(name: str) -> str:
    """``llama3.1:latest`` → ``llama3.1``; the tag is noise in every picker."""
    value = (name or "").strip()
    return value[: -len(":latest")] if value.endswith(":latest") else value


def initials_of(name: str) -> str:
    """ "Phạm Xuân Vinh" → "PV", so the avatar follows whatever name the person chose."""
    parts = [part for part in (name or "").split() if part]
    if not parts:
        return "?"
    letters = parts[0][:2] if len(parts) == 1 else f"{parts[0][0]}{parts[-1][0]}"
    return letters.upper()
