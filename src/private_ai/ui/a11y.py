"""Naming for controls that carry no caption.

A button whose whole label is a glyph is announced as "button" and nothing else: the
tooltip is painted, not spoken. Every icon-only control therefore needs the same sentence
twice, and writing it twice by hand is how fifteen of them ended up with only the tooltip.
``tests/test_a11y.py`` walks the real widgets and fails on any that still do.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from PySide6.QtWidgets import QWidget

__all__ = ["describe"]


def describe(widget: QWidget, text: str) -> None:
    """Give a caption-less control its one sentence, painted and spoken."""
    widget.setToolTip(text)
    widget.setAccessibleName(text)
