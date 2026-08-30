"""Two-step destructive actions, ported from the web app's confirm pattern.

A modal dialog for "delete this conversation" is heavier than the decision deserves; the
first click relabels the button to say what it is about to do, the second does it. The
auto-reset matters as much as the two steps: a button left armed is a trap for the next
person who clicks near it.
"""

from __future__ import annotations

from PySide6.QtCore import QTimer, Signal
from PySide6.QtWidgets import QPushButton, QWidget

from private_ai.ui import icons, theme

__all__ = ["ConfirmButton", "ConfirmToolButton"]

RESET_MS = 4000


class ConfirmButton(QPushButton):
    confirmed = Signal()
    armed = Signal(bool)

    def __init__(
        self,
        text: str,
        confirm_text: str = "",
        *,
        icon_name: str = "",
        danger: bool = True,
        timeout_ms: int = RESET_MS,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(text, parent)
        self._idle_text = text
        self._confirm_text = confirm_text or f"Bấm lại để xác nhận: {text}"
        self._icon_name = icon_name
        self._danger = danger
        self._is_armed = False
        self._timer = QTimer(self)
        self._timer.setSingleShot(True)
        self._timer.setInterval(timeout_ms)
        self._timer.timeout.connect(self.reset)
        self.setProperty("class", "danger" if danger else "")
        self._paint()
        self.clicked.connect(self._advance)

    # ------------------------------------------------------------------ state
    def is_armed(self) -> bool:
        return self._is_armed

    def set_texts(self, text: str, confirm_text: str = "") -> None:
        self._idle_text = text
        self._confirm_text = confirm_text or f"Bấm lại để xác nhận: {text}"
        self._paint()

    def reset(self) -> None:
        if not self._is_armed:
            return
        self._timer.stop()
        self._is_armed = False
        self._paint()
        self.armed.emit(False)

    def _advance(self) -> None:
        if self._is_armed:
            self._timer.stop()
            self._is_armed = False
            self._paint()
            self.armed.emit(False)
            self.confirmed.emit()
            return
        self._is_armed = True
        self._paint()
        self.armed.emit(True)
        self._timer.start()

    def _paint(self) -> None:
        self.setText(self._confirm_text if self._is_armed else self._idle_text)
        self.setToolTip(self._confirm_text if self._is_armed else "")
        if self._icon_name:
            color = theme.token("danger") if (self._is_armed or self._danger) else None
            self.setIcon(icons.icon(self._icon_name, color=color, size=16))
        self.setProperty("class", "danger" if (self._danger or self._is_armed) else "")
        theme.restyle(self)


class ConfirmToolButton(ConfirmButton):
    """Icon-only variant for the row-hover delete affordances in lists."""

    def __init__(
        self,
        icon_name: str = "trash-2",
        *,
        tooltip: str = "Xóa",
        confirm_tooltip: str = "Bấm lại để xác nhận xóa",
        timeout_ms: int = RESET_MS,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(
            "",
            "",
            icon_name=icon_name,
            danger=False,
            timeout_ms=timeout_ms,
            parent=parent,
        )
        self._tooltip = tooltip
        self._confirm_tooltip = confirm_tooltip
        self.setFixedSize(28, 28)
        self.setProperty("class", "icon")
        self._paint()

    def _paint(self) -> None:
        armed = getattr(self, "_is_armed", False)
        self.setText("")
        self.setToolTip(
            getattr(self, "_confirm_tooltip", "") if armed else getattr(self, "_tooltip", "")
        )
        self.setAccessibleName(self.toolTip())
        color = theme.token("danger") if armed else theme.token("faint")
        self.setIcon(icons.icon(self._icon_name or "trash-2", color=color, size=15))
        self.setProperty("class", "icon")
        theme.restyle(self)
