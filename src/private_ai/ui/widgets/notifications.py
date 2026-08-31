"""The notification bell, ported from ``apps/web/src/components/Notifications.tsx``.

Two rules carry the behaviour and are easy to get wrong:

* **Unread is by timestamp, not by acknowledgement.** A notice with an ``at`` is unread
  while it is newer than the last time the panel was closed. A notice *without* one is a
  standing condition ("provider offline"), and those count as unread for as long as the
  problem exists — clicking the bell must not make a real outage look handled.
* **The seen marker is written on close, not on open**, so the highlight stays visible
  while the panel is being read.

``QSettings`` holds the marker (it is per-machine UI chrome, not a synced preference).
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime
from typing import TYPE_CHECKING

from PySide6.QtCore import QPoint, QRectF, QSettings, Qt, Signal
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import icons, theme
from private_ai.ui.format import format_relative_time, notice_tone
from private_ai.ui.widgets.popup import RoundedPopup

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Callable, Sequence

__all__ = ["Notice", "NotificationsButton", "NotificationsPanel"]

SEEN_KEY = "ui/notifications_seen_at"

# A popup, not a page: wide enough for a notice to read as a sentence, narrow enough not
# to cover the view it was raised from.
_PANEL_WIDTH = theme.SPACE["4xl"] * 9 + theme.SPACE["lg"]
# Past this the panel scrolls. Roughly six rows — enough to see there is a list.
_PANEL_MAX_HEIGHT = theme.SPACE["4xl"] * 10 + theme.SPACE["xl"]
_ROW_RADIUS = theme.SPACE["sm"]
_UNREAD_BAR = theme.SPACE["3xs"]
_ICON_SIZE = theme.SPACE["lg"] + theme.SPACE["3xs"]


@dataclass(frozen=True)
class Notice:
    id: str
    title: str
    tone: str = "info"
    detail: str = ""
    # Set for one-off events; status alerts stay unread while the problem lasts.
    at: str = ""
    action_label: str = ""
    action: Callable[[], None] | None = None


class _NoticeRow(QFrame):
    """One notice. The unread marker is painted, not styled.

    It used to be a ``setStyleSheet`` on the row — but ``QLabel`` is a ``QFrame``, so a
    ``QFrame { border-left: … }`` rule matched every label inside it and the panel drew a
    green bracket and a rounded plate around each individual line of text.
    """

    def __init__(self, notice: Notice, unread: bool, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._unread = unread
        spec = notice_tone(notice.tone)

        layout = QHBoxLayout(self)
        # Left inset carries the unread bar; the bar is drawn inside it, not beside it.
        layout.setContentsMargins(
            theme.SPACE["md"], theme.SPACE["sm"], theme.SPACE["md"], theme.SPACE["sm"]
        )
        layout.setSpacing(theme.SPACE["md"])

        mark = QLabel(self)
        mark.setPixmap(icons.pixmap(spec.icon, _ICON_SIZE, theme.token(spec.token)))
        mark.setFixedWidth(theme.SPACE["xl"])
        mark.setAlignment(Qt.AlignmentFlag.AlignTop)
        layout.addWidget(mark)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["2xs"])
        title = QLabel(notice.title, self)
        title.setWordWrap(True)
        title.setProperty("class", "body-strong")
        copy.addWidget(title)
        if notice.detail:
            detail = QLabel(notice.detail, self)
            detail.setWordWrap(True)
            detail.setProperty("class", "muted")
            copy.addWidget(detail)
        if notice.action is not None and notice.action_label:
            action = QPushButton(notice.action_label, self)
            action.setProperty("class", "ghost")
            action.setCursor(Qt.CursorShape.PointingHandCursor)
            # Flush left so it lines up with the copy above it; the accent is the only
            # thing that reads as a link, and the ghost class has no colour of its own.
            action.setStyleSheet(f"QPushButton {{ color: {theme.token('accent')}; padding: 0; }}")
            action.clicked.connect(lambda: notice.action and notice.action())
            copy.addWidget(action, 0, Qt.AlignmentFlag.AlignLeft)
        layout.addLayout(copy, 1)

        if notice.at:
            stamp = QLabel(format_relative_time(notice.at), self)
            stamp.setProperty("class", "faint")
            stamp.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignRight)
            layout.addWidget(stamp)

        self.setAccessibleName(notice.title)
        self.setAccessibleDescription(f"{spec.label}. {notice.detail}".strip())

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        if not self._unread:
            return
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        # ``surface`` is what the panel itself is painted in, so the plate it used to draw
        # was invisible and the bar was carrying the whole marker. ``surface-hover`` is the
        # token for a row picked out of a list, and it keeps ``accent`` for the bar alone.
        painter.setBrush(QColor(theme.token("surface-hover")))
        painter.drawRoundedRect(QRectF(self.rect()), _ROW_RADIUS, _ROW_RADIUS)
        painter.setBrush(QColor(theme.token("accent")))
        painter.drawRoundedRect(
            QRectF(0.0, float(_ROW_RADIUS), float(_UNREAD_BAR), self.height() - 2.0 * _ROW_RADIUS),
            _UNREAD_BAR / 2,
            _UNREAD_BAR / 2,
        )
        painter.end()


class NotificationsPanel(RoundedPopup):
    closed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedWidth(_PANEL_WIDTH)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(*theme.CARD_MARGINS)
        outer.setSpacing(theme.CARD_SPACING)
        head = QHBoxLayout()
        head.setContentsMargins(0, 0, 0, 0)
        head.setSpacing(theme.SPACE["sm"])
        heading = QLabel("Thông báo", self)
        heading.setProperty("class", "heading")
        head.addWidget(heading)
        head.addStretch(1)
        # The bell already carries the count; the panel repeats it because once the panel
        # is open the badge is behind it.
        self._count = QLabel(self)
        self._count.setProperty("class", "muted")
        head.addWidget(self._count)
        outer.addLayout(head)

        self._scroll = QScrollArea(self)
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._scroll.setMaximumHeight(_PANEL_MAX_HEIGHT)
        self._body = QWidget(self._scroll)
        self._list = QVBoxLayout(self._body)
        self._list.setContentsMargins(0, 0, 0, 0)
        self._list.setSpacing(theme.SPACE["2xs"])
        self._list.addStretch(1)
        self._scroll.setWidget(self._body)
        outer.addWidget(self._scroll)

    def set_notices(self, notices: Sequence[Notice], is_unread: Callable[[Notice], bool]) -> None:
        while self._list.count():
            item = self._list.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        unread = sum(1 for notice in notices if is_unread(notice))
        self._count.setText(f"{unread} mục mới" if unread else "")
        self._count.setVisible(bool(unread))
        if not notices:
            empty = QLabel(
                "Mọi thứ đang chạy bình thường. Chưa có gì cần bạn để mắt tới.", self._body
            )
            empty.setWordWrap(True)
            empty.setProperty("class", "empty")
            self._list.addWidget(empty)
        else:
            for notice in notices:
                self._list.addWidget(_NoticeRow(notice, is_unread(notice), self._body))
        self._list.addStretch(1)

    def closeEvent(self, event) -> None:  # noqa: N802 - Qt override
        self.closed.emit()
        super().closeEvent(event)

    def hideEvent(self, event) -> None:  # noqa: N802 - Qt override
        self.closed.emit()
        super().hideEvent(event)


class NotificationsButton(QPushButton):
    opened = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setProperty("class", "icon")
        self.setToolTip("Thông báo")
        self._notices: list[Notice] = []
        self._settings = QSettings("PrivateAI", "PrivateAI")
        self._seen_at = str(self._settings.value(SEEN_KEY, "") or "")
        self._panel: NotificationsPanel | None = None
        self._repaint()
        self.clicked.connect(self._toggle)

    # ------------------------------------------------------------------- API
    def set_notices(self, notices: Sequence[Notice]) -> None:
        self._notices = list(notices)
        self._repaint()
        if self._panel is not None and self._panel.isVisible():
            self._panel.set_notices(self._notices, self._is_unread)

    def unread_count(self) -> int:
        return sum(1 for notice in self._notices if self._is_unread(notice))

    # -------------------------------------------------------------- internals
    def _is_unread(self, notice: Notice) -> bool:
        if notice.at:
            return notice.at > self._seen_at
        return notice.tone == "alert"

    def _repaint(self) -> None:
        unread = self.unread_count()
        color = theme.token("danger") if unread else None
        self.setIcon(icons.icon("bell", color=color, size=19))
        self.setToolTip(f"Thông báo, {unread} mục mới" if unread else "Thông báo")
        self.setAccessibleName(self.toolTip())
        # The badge is drawn as text in the corner rather than as a child widget so it
        # cannot intercept the click.
        self.setText("")
        self.update()

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        super().paintEvent(event)
        unread = self.unread_count()
        if not unread:
            return
        from PySide6.QtGui import QColor, QFont, QPainter

        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        label = "9+" if unread > 9 else str(unread)
        radius = 8 if len(label) == 1 else 10
        inset = theme.SPACE["2xs"]
        rect = self.rect().adjusted(self.width() - 2 * radius - inset, inset, -inset, 0)
        rect.setHeight(2 * radius)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(theme.token("danger")))
        painter.drawRoundedRect(rect, radius, radius)
        font = QFont(self.font())
        font.setPixelSize(theme.type_scale()["2xs"])
        font.setBold(True)
        painter.setFont(font)
        painter.setPen(QColor(theme.token("surface")))
        painter.drawText(rect, Qt.AlignmentFlag.AlignCenter, label)
        painter.end()

    def _toggle(self) -> None:
        if self._panel is not None and self._panel.isVisible():
            self._panel.hide()
            return
        if self._panel is None:
            self._panel = NotificationsPanel(self)
            self._panel.closed.connect(self._mark_seen)
        self.opened.emit()
        self._panel.set_notices(self._notices, self._is_unread)
        self._panel.adjustSize()
        anchor = self.mapToGlobal(
            QPoint(self.width() - self._panel.width(), self.height() + theme.SPACE["sm"])
        )
        self._panel.move(anchor)
        self._panel.show()

    def _mark_seen(self) -> None:
        now = datetime.now(UTC).isoformat()
        self._seen_at = now
        self._settings.setValue(SEEN_KEY, now)
        self._repaint()
