"""Workspace records behind the card grid.

A list model rather than a plain Python list so the filter box can be a
``QSortFilterProxyModel``: the search is purely client-side, exactly as it was in the web
app, and Qt already knows how to keep a filtered view in sync with the source rows.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import QAbstractListModel, QModelIndex, QSortFilterProxyModel, Qt

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.core.schemas import WorkspaceRecord

RECORD_ROLE = int(Qt.ItemDataRole.UserRole) + 1
ID_ROLE = int(Qt.ItemDataRole.UserRole) + 2
SEARCH_ROLE = int(Qt.ItemDataRole.UserRole) + 3


class WorkspacesModel(QAbstractListModel):
    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self._records: list[WorkspaceRecord] = []

    # --- Qt ---------------------------------------------------------------

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
        if role == ID_ROLE:
            return record.id
        if role == SEARCH_ROLE:
            return f"{record.name} {record.description}"
        return None

    # --- data -------------------------------------------------------------

    def set_records(self, records: Sequence[WorkspaceRecord]) -> None:
        self.beginResetModel()
        self._records = list(records)
        self.endResetModel()

    def records(self) -> list[WorkspaceRecord]:
        return list(self._records)

    def record_at(self, row: int) -> WorkspaceRecord | None:
        if 0 <= row < len(self._records):
            return self._records[row]
        return None

    def total_conversations(self) -> int:
        return sum(record.conversation_count for record in self._records)


class WorkspaceFilterProxy(QSortFilterProxyModel):
    """Case-insensitive substring match over name plus description."""

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self._term = ""

    def set_term(self, term: str) -> None:
        self._term = term.strip().casefold()
        self.invalidateFilter()

    def filterAcceptsRow(self, source_row: int, source_parent: QModelIndex) -> bool:  # noqa: N802
        if not self._term:
            return True
        model = self.sourceModel()
        if model is None:
            return True
        haystack = model.data(model.index(source_row, 0, source_parent), SEARCH_ROLE)
        return self._term in str(haystack or "").casefold()

    def record_at(self, row: int):
        return self.data(self.index(row, 0), RECORD_ROLE)
