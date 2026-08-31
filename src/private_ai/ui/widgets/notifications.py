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

from PySide6.QtCore import QPoint, QSettings, Qt, Signal
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
from private_ai.ui.format import format_relative_time

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Callable, Sequence

__all__ = ["Notice", "NotificationsButton", "NotificationsPanel"]

SEEN_KEY = "ui/notifications_seen_at"

_TONE_ICON = {"alert": "alert-triangle", "warn": "info", "info": "check"}
_TONE_TOKEN = {"alert": "danger", "warn": "warn", "info": "success"}

# A popup, not a page: wide enough for a notice to read as a sentence, narrow enough not
# to cover the view it was raised from.
_PANEL_WIDTH = theme.SPACE["4xl"] * 9 + theme.SPACE["lg"]


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
    def __init__(self, notice: Notice, unread: bool, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        tone_color = theme.token(_TONE_TOKEN.get(notice.tone, "success"))
        border = theme.token("accent") if unread else "transparent"
        # The unread bar has no equivalent in the stylesheet, so it is read from the tokens
        # here; every row is rebuilt each time the panel opens, so it cannot go stale.
        self.setStyleSheet(
            f"QFrame {{ border-left: 3px solid {border}; "
            f"background: {theme.token('surface') if unread else 'transparent'}; "
            f"border-radius: 8px; }}"
        )

        layout = QHBoxLayout(self)
        layout.setContentsMargins(
            theme.SPACE["md"], theme.SPACE["sm"], theme.SPACE["md"], theme.SPACE["sm"]
        )
        layout.setSpacing(theme.SPACE["md"])

        mark = QLabel(self)
        mark.setPixmap(icons.pixmap(_TONE_ICON.get(notice.tone, "info"), 18, tone_color))
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


class NotificationsPanel(QFrame):
    closed = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent, Qt.WindowType.Popup)
        self.setProperty("class", "card")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.setFixedWidth(_PANEL_WIDTH)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(*theme.CARD_MARGINS)
        outer.setSpacing(theme.CARD_SPACING)
        heading = QLabel("Thông báo", self)
        heading.setProperty("class", "heading")
        outer.addWidget(heading)

        self._scroll = QScrollArea(self)
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._scroll.setMaximumHeight(420)
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
