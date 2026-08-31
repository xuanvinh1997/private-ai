"""The skeleton every dialog in this package is built on.

Six dialogs had drifted to four different paddings, two header classes and two button
orders. Putting the shape here is what stops a seventh from being invented: a dialog is a
root layout on ``DIALOG_MARGINS``, a title block, its body, and a footer whose primary
action is last.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt
from PySide6.QtWidgets import QHBoxLayout, QLabel, QVBoxLayout

from private_ai.ui.theme import DIALOG_MARGINS, DIALOG_SPACING, SPACE, TOOLBAR_SPACING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from PySide6.QtWidgets import QDialog, QWidget

__all__ = ["action_row", "dialog_layout", "field", "title_block"]


def dialog_layout(dialog: QDialog) -> QVBoxLayout:
    """The root layout of every dialog, so they all start on the same margin."""
    layout = QVBoxLayout(dialog)
    layout.setContentsMargins(*DIALOG_MARGINS)
    layout.setSpacing(DIALOG_SPACING)
    return layout


def title_block(
    layout: QVBoxLayout,
    title: str,
    blurb: str = "",
    trailing: QWidget | None = None,
) -> tuple[QLabel, QLabel]:
    """Title and its one-line explanation as a single block.

    Nested rather than added straight to the root: a full ``DIALOG_SPACING`` between the
    two reads as two unrelated sentences instead of a heading and its subtitle. A
    ``trailing`` widget — a status badge — rides on the title's own line.
    """
    heading = QLabel(title)
    heading.setProperty("class", "title")
    heading.setWordWrap(True)
    subtitle = QLabel(blurb)
    subtitle.setProperty("class", "muted")
    subtitle.setWordWrap(True)

    block = QVBoxLayout()
    block.setSpacing(SPACE["2xs"])
    if trailing is None:
        block.addWidget(heading)
    else:
        row = QHBoxLayout()
        row.setSpacing(TOOLBAR_SPACING)
        row.addWidget(heading, 1)
        row.addWidget(trailing, 0, Qt.AlignmentFlag.AlignTop)
        block.addLayout(row)
    block.addWidget(subtitle)
    layout.addLayout(block)
    return heading, subtitle


def field(layout: QVBoxLayout, text: str, widget: QWidget) -> QLabel:
    """A caption bound to its control, tight enough that the pair reads as one thing.

    The label is returned because some rows are hidden wholesale; a nested layout whose
    widgets are both hidden is empty, so its gap disappears with it.
    """
    label = QLabel(text)
    box = QVBoxLayout()
    box.setSpacing(SPACE["2xs"])
    box.addWidget(label)
    box.addWidget(widget)
    layout.addLayout(box)
    return label


def action_row(layout: QVBoxLayout) -> QHBoxLayout:
    """The footer strip. Callers push a stretch, then the buttons, primary one last."""
    row = QHBoxLayout()
    row.setSpacing(TOOLBAR_SPACING)
    layout.addLayout(row)
    return row
