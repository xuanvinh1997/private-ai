"""Qt item models over ``core.repositories``, and the reconcile they all share.

A poll every 1.2 s must not rebuild a list under the user's cursor: a full
``beginResetModel`` drops the selection, collapses expanded rows and cancels an open
editor. So every model here diffs the incoming page against what it already holds and
emits the narrowest signal that describes the change — the same reason the web app kept
a keyed ``reconcile`` instead of replacing its arrays.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QModelIndex

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from PySide6.QtCore import QAbstractItemModel

__all__ = ["reconcile_rows"]


def _identity(row: dict[str, Any], key: str) -> str:
    return str(row.get(key, ""))


def reconcile_rows(
    model: QAbstractItemModel,
    current: list[dict[str, Any]],
    incoming: Sequence[dict[str, Any]],
    *,
    key: str = "id",
    column_count: int = 1,
) -> None:
    """Bring ``current`` to match ``incoming`` in place, signalling only what moved.

    ``current`` is the list the model reads from; it is mutated between the matching
    ``begin*``/``end*`` calls so Qt's view of it never disagrees with the model's.
    """
    parent = QModelIndex()
    target = [dict(row) for row in incoming]
    surviving = {_identity(row, key) for row in target}

    # Removals first, back to front and in contiguous runs, so no index computed for a
    # later run is invalidated by an earlier one.
    row = len(current) - 1
    while row >= 0:
        if _identity(current[row], key) in surviving:
            row -= 1
            continue
        end = row
        while row >= 0 and _identity(current[row], key) not in surviving:
            row -= 1
        start = row + 1
        model.beginRemoveRows(parent, start, end)
        del current[start : end + 1]
        model.endRemoveRows()

    for position, item in enumerate(target):
        ident = _identity(item, key)
        if position < len(current) and _identity(current[position], key) == ident:
            _patch(model, current, position, item, column_count)
            continue
        source = next(
            (
                candidate
                for candidate in range(position, len(current))
                if _identity(current[candidate], key) == ident
            ),
            -1,
        )
        if source >= 0:
            model.beginMoveRows(parent, source, source, parent, position)
            current.insert(position, current.pop(source))
            model.endMoveRows()
            _patch(model, current, position, item, column_count)
        else:
            model.beginInsertRows(parent, position, position)
            current.insert(position, item)
            model.endInsertRows()

    if len(current) > len(target):
        model.beginRemoveRows(parent, len(target), len(current) - 1)
        del current[len(target) :]
        model.endRemoveRows()


def _patch(
    model: QAbstractItemModel,
    current: list[dict[str, Any]],
    position: int,
    item: dict[str, Any],
    column_count: int,
) -> None:
    if current[position] == item:
        return
    current[position] = item
    left = model.index(position, 0)
    right = model.index(position, max(0, column_count - 1))
    model.dataChanged.emit(left, right)
