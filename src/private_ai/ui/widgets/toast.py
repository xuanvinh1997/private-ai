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
from dataclasses import dataclass

from PySide6.QtCore import QEvent, QObject, QPropertyAnimation, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QPainter
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
from private_ai.ui.format import notice_tone

__all__ = ["Toast", "ToastOverlay", "ToastTone"]

ToastTone = str  # "success" | "info" | "warn" | "error"

LIMIT = 3
LIFETIME_MS = 4500
FADE_MS = 160
# The countdown line repaints on this beat: slow enough to cost nothing, fast enough to
# read as time passing rather than as a progress bar stepping.
TICK_MS = 60

ICON_SIZE = theme.SPACE["xl"]
CLOSE_SIZE = theme.SPACE["lg"]

# One column, not a ragged stack: every toast is the same width, which is also what makes
# the height computable — see ``_reposition``.
MIN_WIDTH = theme.SPACE["4xl"] * 7 + theme.SPACE["xl"]
MAX_WIDTH = theme.SPACE["4xl"] * 10 + theme.SPACE["xl"]


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
        # Width and height are both set by the overlay: a wrapped message is only as tall
        # as its width allows, so the two cannot be decided in different places.

        # One vocabulary with the notification bell: the same event must not be a green
        # tick in one place and a blue circle in the other.
        spec = notice_tone(tone)
        self._tone_color = theme.token(spec.token)
        self.setAccessibleName(title or spec.label)
        self.setAccessibleDescription(message)

        layout = QHBoxLayout(self)
        # Tighter on the right: the close button is an icon control and already carries its
        # own padding.
        layout.setContentsMargins(
            theme.SPACE["lg"], theme.SPACE["md"], theme.SPACE["sm"], theme.SPACE["md"]
        )
        layout.setSpacing(theme.SPACE["md"])

        mark = QLabel(self)
        mark.setPixmap(icons.pixmap(spec.icon, ICON_SIZE, self._tone_color))
        mark.setFixedWidth(ICON_SIZE)
        mark.setAlignment(Qt.AlignmentFlag.AlignTop)
        layout.addWidget(mark)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        # A heading only when the caller has one. The generic one this used to print —
        # "Đã xong", "Có lỗi", "Thông báo" — said what the icon and its colour already say,
        # and made every one-line toast two lines tall.
        if title:
            heading = QLabel(title, self)
            heading.setWordWrap(True)
            heading.setProperty("class", "card-title")
            copy.addWidget(heading)
        body = QLabel(message, self)
        body.setWordWrap(True)
        # The message is the toast's content, not metadata about it.
        body.setProperty("class", "body")
        copy.addWidget(body)
        layout.addLayout(copy, 1)

        close = QPushButton(self)
        close.setProperty("class", "icon")
        close.setIcon(icons.icon("x", size=CLOSE_SIZE))
        close.setToolTip("Đóng thông báo")
        close.setAccessibleName(close.toolTip())
        close.clicked.connect(self._dismiss)
        layout.addWidget(close, 0, Qt.AlignmentFlag.AlignTop)

        self._effect = QGraphicsOpacityEffect(self)
        self.setGraphicsEffect(self._effect)
        self._fade = QPropertyAnimation(self._effect, b"opacity", self)
        self._fade.setDuration(FADE_MS)
        # Connected once, for both directions. Reconnecting per fade meant disconnecting a
        # signal that had nothing on it yet, which libpyside reports as a RuntimeWarning
        # rather than an exception — invisible to the ``suppress`` that was guarding it.
        self._fade.finished.connect(self._faded)
        self._closing = False

        self._timer = QTimer(self)
        self._timer.setSingleShot(True)
        self._timer.setInterval(LIFETIME_MS)
        self._timer.timeout.connect(self._dismiss)

        # Repaints the countdown line, and nothing else; stopped the moment the toast goes.
        self._tick = QTimer(self)
        self._tick.setInterval(TICK_MS)
        self._tick.timeout.connect(self.update)
        self._remaining = 1.0

    def start(self) -> None:
        self._effect.setOpacity(0.0)
        self._fade.stop()
        self._fade.setStartValue(0.0)
        self._fade.setEndValue(1.0)
        self._fade.start()
        self._timer.start()
        self._tick.start()

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        """A hairline counting the toast down, so it does not just vanish mid-sentence."""
        super().paintEvent(event)
        left = self._timer.remainingTime()
        # -1 while the pointer is over the toast and the lifetime timer is stopped, which
        # freezes the line exactly where it was — what the pause looks like.
        if left >= 0:
            self._remaining = max(0.0, min(1.0, left / LIFETIME_MS))
        if self._closing or self._remaining <= 0.0:
            return
        inset = theme.SPACE["lg"]
        thickness = theme.SPACE["3xs"]
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        color = QColor(self._tone_color)
        # Paused reads as dimmer, not as stopped-and-broken.
        color.setAlpha(150 if self._timer.isActive() else 80)
        painter.setBrush(color)
        painter.drawRoundedRect(
            QRectF(
                inset,
                self.height() - thickness - theme.SPACE["2xs"],
                (self.width() - 2 * inset) * self._remaining,
                thickness,
            ),
            thickness / 2,
            thickness / 2,
        )
        painter.end()

    def enterEvent(self, event: QEvent) -> None:  # noqa: N802 - Qt override
        # pauseOnInteraction in the web build: a toast the user is reading must not vanish.
        self._timer.stop()
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event: QEvent) -> None:  # noqa: N802 - Qt override
        if not self._closing:
            self._timer.start()
        super().leaveEvent(event)

    def _dismiss(self) -> None:
        # The close button and the lifetime timer both land here, and so does a second
        # click during the fade.
        if self._closing:
            return
        self._closing = True
        self._timer.stop()
        self._tick.stop()
        self._fade.stop()
        self._fade.setStartValue(self._effect.opacity())
        self._fade.setEndValue(0.0)
        self._fade.start()

    def _faded(self) -> None:
        """The one handler for both fades; only the fade-out retires the toast."""
        if self._closing:
            self.dismissed.emit(self)


class ToastOverlay(QWidget):
    """Positions and queues the toasts. One per window; ``show_toast`` is the whole API."""

    def __init__(self, parent: QWidget) -> None:
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, False)
        self.setObjectName("ToastOverlay")
        self._layout = QVBoxLayout(self)
        self._layout.setContentsMargins(0, 0, 0, 0)
        self._layout.setSpacing(theme.SPACE["sm"])
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
        pending = _Pending(text, tone, (title or "").strip())
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
        # No alignment flag: Qt skips heightForWidth for any layout item that carries one,
        # so an aligned toast was given its unwrapped height and clipped its own message.
        # The overlay is already anchored to the window's bottom-right corner.
        self._layout.addWidget(toast)
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
        inset = theme.SPACE["2xl"]
        width = min(MAX_WIDTH, max(MIN_WIDTH, parent.width() - inset * 2))
        self.setFixedWidth(width)
        # Each toast is measured at the width it will actually be painted at. sizeHint()
        # answers for the layout's *preferred* width, which for a wrapped message is the
        # unwrapped one — that is how a two-line error came out clipped to one and a half.
        height = 0
        for index, toast in enumerate(self._live):
            # heightForWidth, never sizeHint: the hint answers for the width the message
            # would like (one long line), so using it left a short toast 20px of dead air
            # that the two labels then shared between them.
            needed = (
                toast.heightForWidth(width)
                if toast.hasHeightForWidth()
                else toast.sizeHint().height()
            )
            toast.setFixedHeight(max(1, needed))
            height += toast.height() + (self._layout.spacing() if index else 0)
        height = max(1, height)
        self.setGeometry(
            parent.width() - width - inset, parent.height() - height - inset, width, height
        )
        self.raise_()
        self.setVisible(bool(self._live))

    def eventFilter(self, watched: QObject, event: QEvent) -> bool:  # noqa: N802 - Qt override
        if watched is self.parentWidget() and event.type() in (
            QEvent.Type.Resize,
            QEvent.Type.Show,
        ):
            self._reposition()
        return False
