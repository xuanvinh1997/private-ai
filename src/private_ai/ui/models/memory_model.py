"""Saved memories behind the memory list."""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import QAbstractListModel, QModelIndex, Qt

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.core.schemas import MemoryRecord

RECORD_ROLE = int(Qt.ItemDataRole.UserRole) + 1
ID_ROLE = int(Qt.ItemDataRole.UserRole) + 2

TYPE_LABELS: dict[str, str] = {
    "preference": "Sở thích",
    "fact": "Thông tin",
    "episodic": "Phiên làm việc",
}


def type_label(value: object) -> str:
    key = str(getattr(value, "value", value) or "")
    return TYPE_LABELS.get(key, key or "Khác")


class MemoryModel(QAbstractListModel):
    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self._records: list[MemoryRecord] = []

    def rowCount(self, parent: QModelIndex | None = None) -> int:  # noqa: N802
        if parent is not None and parent.isValid():
            return 0
        return len(self._records)

    def data(self, index: QModelIndex, role: int = Qt.ItemDataRole.DisplayRole):
        if not index.isValid() or not 0 <= index.row() < len(self._records):
            return None
        record = self._records[index.row()]
        if role in (Qt.ItemDataRole.DisplayRole, Qt.ItemDataRole.EditRole):
            return record.content
        if role == RECORD_ROLE:
            return record
        if role == ID_ROLE:
            return record.id
        return None

    def set_records(self, records: Sequence[MemoryRecord]) -> None:
        self.beginResetModel()
        self._records = list(records)
        self.endResetModel()

    def records(self) -> list[MemoryRecord]:
        return list(self._records)

    def record_by_id(self, memory_id: str) -> MemoryRecord | None:
        return next((item for item in self._records if item.id == memory_id), None)

    def export_payload(self) -> list[dict]:
        """What "Xuất JSON" writes: the record as stored, dates as ISO strings."""
        return [record.model_dump(mode="json") for record in self._records]
