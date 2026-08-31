"""Title, on-device status, context-rail toggle, notifications.

The "Trên thiết bị" line under the title is not decoration: it is the app's central
promise, and the pip beside it is the only place the user learns the local runtime has
gone away before a chat turn fails.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import QHBoxLayout, QLabel, QPushButton, QVBoxLayout, QWidget

from private_ai.ui import icons, theme
from private_ai.ui.widgets.notifications import NotificationsButton
from private_ai.ui.widgets.status_pip import StatusPip

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.ui.widgets.notifications import Notice

__all__ = ["Topbar"]


class Topbar(QWidget):
    contextToggled = Signal(bool)
    notificationsOpened = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName("Topbar")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        # 48px of content between two 16px bands puts the controls on the same baseline as
        # the sidebar header's toggle, which is the widget on the other side of the seam.
        # The 24px inset is the page margin, so the title starts where the view below it does.
        self.setFixedHeight(theme.SPACE["4xl"] + theme.SPACE["xl"] * 2)

        row = QHBoxLayout(self)
        row.setContentsMargins(
            theme.SPACE["2xl"], theme.SPACE["lg"], theme.SPACE["2xl"], theme.SPACE["lg"]
        )
        row.setSpacing(theme.TOOLBAR_SPACING)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        self._title = QLabel(self)
        self._title.setProperty("class", "body-strong")
        copy.addWidget(self._title)

        status = QHBoxLayout()
        status.setContentsMargins(0, 0, 0, 0)
        status.setSpacing(theme.SPACE["xs"])
        self._pip = StatusPip("online", self)
        self._where = QLabel("Trên thiết bị", self)
        self._where.setProperty("class", "muted")
        status.addWidget(self._pip)
        status.addWidget(self._where)
        status.addStretch(1)
        copy.addLayout(status)
        row.addLayout(copy, 1)

        self.context_toggle = QPushButton(self)
        self.context_toggle.setProperty("class", "icon")
        self.context_toggle.setCheckable(True)
        self.context_toggle.setChecked(True)
        self.context_toggle.setIcon(icons.icon("panel-right-close", size=18))
        self.context_toggle.setToolTip("Ẩn bảng ngữ cảnh")
        self.context_toggle.toggled.connect(self._on_context_toggled)
        row.addWidget(self.context_toggle)

        self.notifications = NotificationsButton(self)
        self.notifications.opened.connect(self.notificationsOpened)
        row.addWidget(self.notifications)

    # -------------------------------------------------------------------- API
    def set_title(self, title: str) -> None:
        self._title.setText(title)

    def set_status(self, state: str, label: str = "Trên thiết bị") -> None:
        self._pip.set_state(state)
        self._where.setText(label)

    def set_notices(self, notices: Sequence[Notice]) -> None:
        self.notifications.set_notices(notices)

    def set_context_visible(self, visible: bool) -> None:
        """Only the chat screen has a context rail; every other view hides the toggle."""
        self.context_toggle.setVisible(visible)

    def set_context_open(self, open_: bool) -> None:
        if self.context_toggle.isChecked() != open_:
            self.context_toggle.setChecked(open_)

    def _on_context_toggled(self, checked: bool) -> None:
        self.context_toggle.setIcon(
            icons.icon("panel-right-close" if checked else "panel-right-open", size=18)
        )
        self.context_toggle.setToolTip("Ẩn bảng ngữ cảnh" if checked else "Hiện bảng ngữ cảnh")
        self.contextToggled.emit(checked)
