"""The conversation list in the sidebar, fed by ``repositories.list_conversations``.

Same reconcile as the documents table, for a sharper reason: this list re-sorts on every
answer (``updated_at`` bumps the conversation to the top), and a reset in the middle of a
turn would drop the highlight off the conversation the user is reading.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QAbstractListModel, QModelIndex, Qt

from private_ai.core.repositories import DEFAULT_CONVERSATION_TITLE
from private_ai.ui.format import format_relative_time
from private_ai.ui.models import reconcile_rows

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from PySide6.QtCore import QObject

    from private_ai.core.schemas import ConversationRecord

__all__ = ["ConversationsModel"]

IdRole = Qt.ItemDataRole.UserRole + 1
RecordRole = Qt.ItemDataRole.UserRole + 2
SubtitleRole = Qt.ItemDataRole.UserRole + 3
MessageCountRole = Qt.ItemDataRole.UserRole + 4
ModelRole = Qt.ItemDataRole.UserRole + 5


def _as_row(item: ConversationRecord | dict[str, Any]) -> dict[str, Any]:
    if isinstance(item, dict):
        return dict(item)
    return {
        "id": item.id,
        "workspace_id": item.workspace_id,
        "title": item.title,
        "model": item.model or "",
        "message_count": item.message_count,
        "created_at": item.created_at.isoformat(),
        "updated_at": item.updated_at.isoformat(),
    }


class ConversationsModel(QAbstractListModel):
    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._rows: list[dict[str, Any]] = []

    def rowCount(self, parent: QModelIndex | None = None) -> int:  # noqa: N802
        if parent is not None and parent.isValid():
            return 0
        return len(self._rows)

    def data(self, index: QModelIndex, role: int = Qt.ItemDataRole.DisplayRole) -> Any:
        if not index.isValid() or not 0 <= index.row() < len(self._rows):
            return None
        row = self._rows[index.row()]
        if role in (Qt.ItemDataRole.DisplayRole, Qt.ItemDataRole.EditRole):
            return str(row.get("title") or DEFAULT_CONVERSATION_TITLE)
        if role == Qt.ItemDataRole.ToolTipRole:
            return self._subtitle(row)
        if role == SubtitleRole:
            return self._subtitle(row)
        if role == IdRole:
            return str(row.get("id") or "")
        if role == RecordRole:
            return row
        if role == MessageCountRole:
            return int(row.get("message_count") or 0)
        if role == ModelRole:
            return str(row.get("model") or "")
        return None

    @staticmethod
    def _subtitle(row: dict[str, Any]) -> str:
        count = int(row.get("message_count") or 0)
        when = format_relative_time(str(row.get("updated_at") or ""))
        return f"{count} tin nhắn · {when}" if count else when

    # --- data ---------------------------------------------------------------

    def update_rows(self, rows: Sequence[ConversationRecord | dict[str, Any]]) -> None:
        reconcile_rows(self, self._rows, [_as_row(row) for row in rows], key="id", column_count=1)

    def rows(self) -> list[dict[str, Any]]:
        return list(self._rows)

    def record(self, row: int) -> dict[str, Any] | None:
        return self._rows[row] if 0 <= row < len(self._rows) else None

    def conversation_id(self, row: int) -> str:
        record = self.record(row)
        return str(record.get("id") or "") if record else ""

    def row_for(self, conversation_id: str) -> int:
        return next(
            (
                position
                for position, row in enumerate(self._rows)
                if str(row.get("id") or "") == conversation_id
            ),
            -1,
        )
