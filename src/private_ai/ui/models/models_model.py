"""Installed models behind the model list.

``ModelInfo`` carries no notion of which task a model is the default for — that lives in
``model_defaults`` — so the model joins the two and hands each row a ready-made
``default_for`` list, which is what the per-task badge renders.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import QAbstractListModel, QModelIndex, Qt

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Mapping, Sequence

    from private_ai.core.schemas import ModelInfo

RECORD_ROLE = int(Qt.ItemDataRole.UserRole) + 1
NAME_ROLE = int(Qt.ItemDataRole.UserRole) + 2

STATE_LABELS: dict[str, str] = {
    "installed": "Đã cài",
    "loaded": "Đang nạp",
    "unloaded": "Chưa nạp",
    "downloading": "Đang tải",
    "failed": "Lỗi",
}

# The shell's StatusPip vocabulary, keyed by model state.
STATE_PIPS: dict[str, str] = {
    "loaded": "online",
    "installed": "ok",
    "unloaded": "warn",
    "downloading": "busy",
    "failed": "error",
}

TASK_LABELS: dict[str, str] = {
    "chat": "Trò chuyện",
    "embedding": "Embedding",
    "vision": "OCR",
    "asr": "Giọng nói",
}


def state_label(state: object) -> str:
    key = str(getattr(state, "value", state) or "")
    return STATE_LABELS.get(key, key or "Không rõ")


def state_pip(state: object) -> str:
    key = str(getattr(state, "value", state) or "")
    return STATE_PIPS.get(key, "unknown")


def initials_of(name: str) -> str:
    """ "qwen3:8b" -> "Q8" — the two-letter glyph the web app drew beside each row."""
    parts = [part for part in name.replace("/", " ").replace(":", " ").replace("_", " ").split()]
    if not parts:
        return "?"
    if len(parts) == 1:
        return parts[0][:2].upper()
    return f"{parts[0][0]}{parts[-1][0]}".upper()


class ModelsModel(QAbstractListModel):
    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self._records: list[ModelInfo] = []
        self._defaults: dict[str, str] = {}

    def rowCount(self, parent: QModelIndex | None = None) -> int:  # noqa: N802
        if parent is not None and parent.isValid():
            return 0
        return len(self._records)

    def data(self, index: QModelIndex, role: int = Qt.ItemDataRole.DisplayRole):
        if not index.isValid() or not 0 <= index.row() < len(self._records):
            return None
        record = self._records[index.row()]
        if role in (Qt.ItemDataRole.DisplayRole, Qt.ItemDataRole.EditRole):
            return record.name
        if role == RECORD_ROLE:
            return record
        if role == NAME_ROLE:
            return record.name
        return None

    def set_records(
        self,
        records: Sequence[ModelInfo],
        defaults: Mapping[str, str] | None = None,
    ) -> None:
        self._defaults = {str(k): str(v) for k, v in (defaults or {}).items()}
        decorated = []
        for record in records:
            tasks = sorted(task for task, name in self._defaults.items() if name == record.name)
            decorated.append(record.model_copy(update={"default_for": tasks}))
        self.beginResetModel()
        self._records = decorated
        self.endResetModel()

    def records(self) -> list[ModelInfo]:
        return list(self._records)

    def defaults(self) -> dict[str, str]:
        return dict(self._defaults)

    def chat_models(self) -> list[ModelInfo]:
        """Everything that can answer a prompt — what the graph-extraction picker offers."""
        return [
            record
            for record in self._records
            if record.model_type != "embedding" and "embedding" not in record.capabilities
        ]
