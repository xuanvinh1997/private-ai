"""The one rounded surface that is also a window.

A ``Qt.Popup`` is not a widget on a page — it is a window of its own, and the platform
fills that window's rectangle before Qt paints anything into it. So a card with
``border-radius: 14px`` came out rounded *inside* a square block of surface colour: the
spare square border every popup in the app was carrying, most visible against a dark
desktop where the block was a pale frame around the panel.

Making the window translucent removes the block, but a translucent top-level does not get
its background from the application stylesheet at all — measured on macOS: the same frame
paints nothing with the app's ``QFrame[class="card"]`` rule, and paints correctly with a
stylesheet of its own or with its own ``paintEvent``. This paints, so the card reads its
colours from the tokens at paint time and follows a theme change with no rebuild.
"""

from __future__ import annotations

from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import QFrame, QWidget

from private_ai.ui import theme

__all__ = ["RoundedPopup"]


class RoundedPopup(QFrame):
    """A popup that is only its card: no window rectangle behind the rounded corners."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent, Qt.WindowType.Popup)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        # Qt.Popup is frameless on most platforms already; saying so is what stops the
        # platform drawing chrome behind a shape it knows nothing about. The drop shadow is
        # kept: on a translucent window it follows the rounded outline, not the rectangle.
        self.setWindowFlag(Qt.WindowType.FramelessWindowHint, True)

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(QColor(theme.token("line")))
        painter.setBrush(QColor(theme.token("surface")))
        # Half a pixel in on every side, or the 1px border is drawn half outside the widget
        # and comes out as a soft grey smear instead of a line.
        painter.drawRoundedRect(
            QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5),
            theme.CARD_RADIUS,
            theme.CARD_RADIUS,
        )
        painter.end()
