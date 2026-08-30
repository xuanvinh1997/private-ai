"""The document table, fed by ``repositories.list_documents``.

The rows are the dicts that repository returns verbatim, ``ingestion`` sub-dict and all,
so nothing is lost between the query and a view that wants the embedding rate. Only the
display columns are derived here.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QAbstractTableModel, QModelIndex, Qt

from private_ai.ui.format import format_file_size, format_relative_time, stage_label, status_label
from private_ai.ui.models import reconcile_rows

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from PySide6.QtCore import QObject

__all__ = [
    "COLUMN_FILENAME",
    "COLUMN_SIZE",
    "COLUMN_STATUS",
    "COLUMN_UPDATED",
    "DocumentsModel",
    "document_progress",
    "document_status_text",
    "is_document_busy",
]

COLUMN_FILENAME = 0
COLUMN_STATUS = 1
COLUMN_SIZE = 2
COLUMN_UPDATED = 3

_HEADERS = ("Tên tệp", "Trạng thái", "Dung lượng", "Cập nhật")

# Fixed points, matching the web app: a document with no job row still has to place its
# bar somewhere, and these are the places its status means.
_STATUS_PROGRESS = {
    "queued": 0.45,
    "processing": 0.78,
    "ready": 1.0,
    "needs_ocr": 1.0,
    "failed": 1.0,
}

_BUSY_STATUSES = frozenset({"queued", "processing"})

IdRole = Qt.ItemDataRole.UserRole + 1
RecordRole = Qt.ItemDataRole.UserRole + 2
StatusRole = Qt.ItemDataRole.UserRole + 3
ProgressRole = Qt.ItemDataRole.UserRole + 4
BusyRole = Qt.ItemDataRole.UserRole + 5
DetailRole = Qt.ItemDataRole.UserRole + 6


def is_document_busy(document: dict[str, Any]) -> bool:
    ingestion = document.get("ingestion") or {}
    if str(ingestion.get("status") or "") == "processing":
        return True
    return str(document.get("status") or "") in _BUSY_STATUSES


def document_progress(document: dict[str, Any]) -> float:
    ingestion = document.get("ingestion") or {}
    if ingestion:
        return max(0.0, min(1.0, float(ingestion.get("progress") or 0.0)))
    status = str(document.get("status") or "")
    # Extracted but not yet indexed is real, partial work — not a finished document.
    if status == "ready" and not document.get("indexed_at"):
        return 0.42
    return _STATUS_PROGRESS.get(status, 0.0)


def document_status_text(document: dict[str, Any]) -> str:
    """What the row says under the filename: the live stage while working, else the status."""
    ingestion = document.get("ingestion") or {}
    if str(ingestion.get("status") or "") == "processing":
        detail = str(ingestion.get("detail") or "")
        return detail or stage_label(str(ingestion.get("step") or "processing"))
    return status_label(str(document.get("status") or ""))


class DocumentsModel(QAbstractTableModel):
    """Documents of one workspace. Refreshed by diff so a 1.2 s poll is invisible."""

    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._rows: list[dict[str, Any]] = []
        self.total = 0

    # --- Qt -----------------------------------------------------------------

    def rowCount(self, parent: QModelIndex | None = None) -> int:  # noqa: N802
        if parent is not None and parent.isValid():
            return 0
        return len(self._rows)

    def columnCount(self, parent: QModelIndex | None = None) -> int:  # noqa: N802
        if parent is not None and parent.isValid():
            return 0
        return len(_HEADERS)

    def headerData(  # noqa: N802
        self,
        section: int,
        orientation: Qt.Orientation,
        role: int = Qt.ItemDataRole.DisplayRole,
    ) -> Any:
        if role != Qt.ItemDataRole.DisplayRole or orientation != Qt.Orientation.Horizontal:
            return None
        return _HEADERS[section] if 0 <= section < len(_HEADERS) else None

    def data(self, index: QModelIndex, role: int = Qt.ItemDataRole.DisplayRole) -> Any:
        if not index.isValid() or not 0 <= index.row() < len(self._rows):
            return None
        row = self._rows[index.row()]
        if role == Qt.ItemDataRole.DisplayRole:
            return self._display(row, index.column())
        if role == Qt.ItemDataRole.ToolTipRole:
            return str(row.get("error") or row.get("filename") or "")
        if role == IdRole:
            return str(row.get("id") or "")
        if role == RecordRole:
            return row
        if role == StatusRole:
            return str(row.get("status") or "")
        if role == ProgressRole:
            return document_progress(row)
        if role == BusyRole:
            return is_document_busy(row)
        if role == DetailRole:
            return document_status_text(row)
        return None

    @staticmethod
    def _display(row: dict[str, Any], column: int) -> str:
        if column == COLUMN_FILENAME:
            return str(row.get("filename") or "")
        if column == COLUMN_STATUS:
            return document_status_text(row)
        if column == COLUMN_SIZE:
            return format_file_size(int(row.get("byte_size") or 0))
        if column == COLUMN_UPDATED:
            return format_relative_time(str(row.get("updated_at") or ""))
        return ""

    # --- data ---------------------------------------------------------------

    def update_rows(self, rows: Sequence[dict[str, Any]], *, total: int | None = None) -> None:
        reconcile_rows(self, self._rows, rows, key="id", column_count=len(_HEADERS))
        if total is not None:
            self.total = total

    def rows(self) -> list[dict[str, Any]]:
        return list(self._rows)

    def record(self, row: int) -> dict[str, Any] | None:
        return self._rows[row] if 0 <= row < len(self._rows) else None

    def document_id(self, row: int) -> str:
        record = self.record(row)
        return str(record.get("id") or "") if record else ""

    def row_for(self, document_id: str) -> int:
        return next(
            (
                position
                for position, row in enumerate(self._rows)
                if str(row.get("id") or "") == document_id
            ),
            -1,
        )

    def any_busy(self) -> bool:
        """Whether the caller still has a reason to keep polling."""
        return any(is_document_busy(row) for row in self._rows)
