"""Transient feedback, stacked bottom-right over the main window.

Kobalte's toaster gave the web app a queue with a limit of 3 and a 4.5s life; both numbers
are reproduced here because they are what makes a burst of failures readable instead of a
wall. Anything past the limit waits rather than being dropped — a swallowed error message
is worse than a late one.

The overlay is a child of the window rather than a top-level window: a real popup steals
focus on Windows and shows in the taskbar on some Linux window managers.
"""

from __future__ import annotations

from collections import deque
from contextlib import suppress
from dataclasses import dataclass

from PySide6.QtCore import QEvent, QObject, QPropertyAnimation, Qt, QTimer, Signal
from PySide6.QtWidgets import (
    QFrame,
    QGraphicsOpacityEffect,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import icons, theme

__all__ = ["Toast", "ToastOverlay", "ToastTone"]

ToastTone = str  # "success" | "error" | "info"

LIMIT = 3
LIFETIME_MS = 4500
FADE_MS = 160

_ICON = {"success": "check", "error": "alert-triangle", "info": "info"}
_TONE_TOKEN = {"success": "success", "error": "danger", "info": "accent"}
_TITLE = {"success": "Đã xong", "error": "Có lỗi", "info": "Thông báo"}


@dataclass(frozen=True)
class _Pending:
    message: str
    tone: str
    title: str


class Toast(QFrame):
    dismissed = Signal(object)

    def __init__(self, message: str, tone: str, title: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setProperty("class", "card")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.setMinimumWidth(280)
        self.setMaximumWidth(420)

        tone_color = theme.token(_TONE_TOKEN.get(tone, "accent"))
        layout = QHBoxLayout(self)
        layout.setContentsMargins(13, 11, 9, 11)
        layout.setSpacing(10)

        mark = QLabel(self)
        mark.setPixmap(icons.pixmap(_ICON.get(tone, "info"), 19, tone_color))
        mark.setFixedWidth(20)
        mark.setAlignment(Qt.AlignmentFlag.AlignTop)
        layout.addWidget(mark)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(2)
        heading = QLabel(title, self)
        heading.setStyleSheet(f"color: {tone_color}; font-weight: 700;")
        body = QLabel(message, self)
        body.setWordWrap(True)
        body.setProperty("class", "muted")
        copy.addWidget(heading)
        copy.addWidget(body)
        layout.addLayout(copy, 1)

        close = QPushButton(self)
        close.setProperty("class", "icon")
        close.setIcon(icons.icon("x", size=15))
        close.setFixedSize(26, 26)
        close.setToolTip("Đóng thông báo")
        close.clicked.connect(self._dismiss)
        layout.addWidget(close, 0, Qt.AlignmentFlag.AlignTop)

        self._effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self._effect)
        self._fade = QPropertyAnimation(self._effect, b"opacity", self)
        self._fade.setDuration(FADE_MS)

        self._timer = QTimer(self)
        self._timer.setSingleShot(True)
        self._timer.setInterval(LIFETIME_MS)
        self._timer.timeout.connect(self._dismiss)

    def start(self) -> None:
        self._effect.setOpacity(0.0)
        self._fade.stop()
        self._fade.setStartValue(0.0)
        self._fade.setEndValue(1.0)
        self._fade.start()
        self._timer.start()

    def enterEvent(self, event: QEvent) -> None:  # noqa: N802 - Qt override
        # pauseOnInteraction in the web build: a toast the user is reading must not vanish.
        self._timer.stop()
        super().enterEvent(event)

    def leaveEvent(self, event: QEvent) -> None:  # noqa: N802 - Qt override
        self._timer.start()
        super().leaveEvent(event)

    def _dismiss(self) -> None:
        self._timer.stop()
        self._fade.stop()
        self._fade.setStartValue(self._effect.opacity())
        self._fade.setEndValue(0.0)
        # Disconnected first: the animation object is reused and would otherwise fire the
        # previous fade-in's handler too.
        with suppress(RuntimeError, TypeError):
            self._fade.finished.disconnect()
        self._fade.finished.connect(lambda: self.dismissed.emit(self))
        self._fade.start()


class ToastOverlay(QWidget):
    """Positions and queues the toasts. One per window; ``show_toast`` is the whole API."""

    def __init__(self, parent: QWidget) -> None:
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, False)
        self.setObjectName("ToastOverlay")
        self._layout = QVBoxLayout(self)
        self._layout.setContentsMargins(0, 0, 0, 0)
        self._layout.setSpacing(9)
        self._layout.addStretch(1)
        self._live: list[Toast] = []
        self._queue: deque[_Pending] = deque()
        parent.installEventFilter(self)
        self._reposition()

    # ------------------------------------------------------------------- API
    def show_toast(self, message: str, tone: str = "info", title: str = "") -> None:
        text = (message or "").strip()
        if not text:
            return
        tone = tone if tone in _ICON else "info"
        pending = _Pending(text, tone, title or _TITLE.get(tone, "Thông báo"))
        if len(self._live) >= LIMIT:
            self._queue.append(pending)
            return
        self._spawn(pending)

    def clear(self) -> None:
        self._queue.clear()
        for toast in list(self._live):
            self._remove(toast)

    # -------------------------------------------------------------- internals
    def _spawn(self, pending: _Pending) -> None:
        toast = Toast(pending.message, pending.tone, pending.title, self)
        toast.dismissed.connect(self._remove)
        self._layout.addWidget(toast, 0, Qt.AlignmentFlag.AlignRight)
        self._live.append(toast)
        toast.show()
        toast.start()
        self._reposition()

    def _remove(self, toast: Toast) -> None:
        if toast in self._live:
            self._live.remove(toast)
        self._layout.removeWidget(toast)
        toast.setParent(None)
        toast.deleteLater()
        if self._queue and len(self._live) < LIMIT:
            self._spawn(self._queue.popleft())
        self._reposition()

    def _reposition(self) -> None:
        parent = self.parentWidget()
        if parent is None:
            return
        width = min(440, max(300, parent.width() - 48))
        self.setFixedWidth(width)
        self.adjustSize()
        height = max(1, self.sizeHint().height())
        self.setGeometry(parent.width() - width - 24, parent.height() - height - 24, width, height)
        self.raise_()
        self.setVisible(bool(self._live))

    def eventFilter(self, watched: QObject, event: QEvent) -> bool:  # noqa: N802 - Qt override
        if watched is self.parentWidget() and event.type() in (
            QEvent.Type.Resize,
            QEvent.Type.Show,
        ):
            self._reposition()
        return False
