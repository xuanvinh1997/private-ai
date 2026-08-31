"""The sidebar footer: who you are, and how to become someone else.

Profiles are local identities that own separate memory but share documents and
workspaces; the copy says so, because "add profile" otherwise reads like an account.
Ported from ``apps/web/src/components/Profiles.tsx``.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import QPoint, QSize, Qt, Signal
from PySide6.QtGui import QIcon, QPixmap
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import icons, theme
from private_ai.ui.format import initials_of
from private_ai.ui.widgets.confirm_button import ConfirmButton
from private_ai.ui.widgets.status_pip import StatusPip

MENU_ICON_PX = 15
# ``QPushButton[class="menu-item"]`` pads its contents by this much, and the heading above
# the rows has to start on the same edge.
MENU_TEXT_INSET = 10


def _menu_row(text: str, icon_name: str, parent: QWidget) -> QPushButton:
    """One popup row, left-aligned with a fixed icon slot.

    Every row reserves the same icon width even when it has no icon, because a row without
    one would otherwise start its caption where the others start their glyph — which is
    what made this menu fan out around its centre line instead of reading as a column.
    """
    button = QPushButton(text, parent)
    button.setProperty("class", "menu-item")
    button.setIconSize(QSize(MENU_ICON_PX, MENU_ICON_PX))
    if icon_name:
        button.setIcon(icons.icon(icon_name, size=MENU_ICON_PX))
    else:
        blank = QPixmap(MENU_ICON_PX, MENU_ICON_PX)
        blank.fill(Qt.GlobalColor.transparent)
        button.setIcon(QIcon(blank))
    return button


if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.ui.context import AppContext

__all__ = ["ProfileSwitcher"]


class ProfileSwitcher(QWidget):
    settingsRequested = Signal()
    profileChanged = Signal(str)

    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.ctx = ctx
        self._profiles: list[object] = []
        self._active_id = ""
        self._active_name = "Bạn"
        self._menu: QFrame | None = None

        # The card is drawn by the panel around the row rather than by the row itself: a
        # QPushButton cannot carry a container's styling without an inline sheet that would
        # keep the old palette after a theme switch.
        self.setProperty("class", "panel")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(*(theme.SPACE["2xs"],) * 4)
        self._button = QPushButton(self)
        self._button.setProperty("class", "nav-item")
        self._button.setCursor(Qt.CursorShape.PointingHandCursor)
        self._button.clicked.connect(self._open_menu)
        layout.addWidget(self._button, 1)

        inner = QHBoxLayout(self._button)
        # A nav row is pinned to 40px, so the avatar and the two lines of copy take all of
        # the height and the padding is horizontal only.
        inner.setContentsMargins(theme.SPACE["sm"], 0, theme.SPACE["sm"], 0)
        inner.setSpacing(theme.SPACE["sm"])
        self._avatar = QLabel(self._button)
        self._avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._avatar.setProperty("class", "avatar-lg")
        inner.addWidget(self._avatar)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        self._name = QLabel(self._active_name, self._button)
        self._name.setProperty("class", "body-strong")
        status = QHBoxLayout()
        status.setContentsMargins(0, 0, 0, 0)
        status.setSpacing(theme.SPACE["xs"])
        self._pip = StatusPip("online", self._button)
        self._where = QLabel("Trên thiết bị", self._button)
        self._where.setProperty("class", "muted")
        status.addWidget(self._pip)
        status.addWidget(self._where, 1)
        copy.addWidget(self._name)
        copy.addLayout(status)
        inner.addLayout(copy, 1)

        caret = QLabel(self._button)
        caret.setPixmap(icons.pixmap("chevrons-up-down", 15, theme.token("muted")))
        inner.addWidget(caret)

        self._render()

    # ------------------------------------------------------------------- API
    def set_online(self, online: bool) -> None:
        self._pip.set_state("online" if online else "offline")

    def refresh(self) -> None:
        from private_ai.core import repositories

        def loaded(profiles) -> None:
            self._profiles = list(profiles)
            active = next((p for p in self._profiles if getattr(p, "active", False)), None)
            if active is None and self._profiles:
                active = self._profiles[0]
            if active is not None:
                self._active_id = str(getattr(active, "id", ""))
                self._active_name = (getattr(active, "display_name", "") or "").strip() or "Bạn"
            self._render()

        self.ctx.run(repositories.list_profiles(self.ctx.database), on_result=loaded)

    def active_id(self) -> str:
        return self._active_id

    def active_name(self) -> str:
        return self._active_name

    # -------------------------------------------------------------- internals
    def _render(self) -> None:
        self._avatar.setText(initials_of(self._active_name))
        self._name.setText(self._active_name)
        self._button.setAccessibleName(f"Hồ sơ {self._active_name}")
        self._button.setToolTip(f"Hồ sơ {self._active_name}")

    def _open_menu(self) -> None:
        if self._menu is not None and self._menu.isVisible():
            self._menu.hide()
            return
        menu = QFrame(self, Qt.WindowType.Popup)
        menu.setProperty("class", "card")
        menu.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        menu.setMinimumWidth(max(250, self.width()))
        box = QVBoxLayout(menu)
        box.setContentsMargins(*(theme.SPACE["sm"],) * 4)
        box.setSpacing(theme.SPACE["3xs"])

        heading = QLabel("Hồ sơ trên máy này", menu)
        heading.setProperty("class", "section-label")
        # The rows carry the stylesheet's own horizontal padding, so the heading is
        # indented by the same amount rather than starting a column of its own.
        heading.setContentsMargins(MENU_TEXT_INSET, 0, 0, 0)
        box.addWidget(heading)

        for profile in self._profiles:
            pid = str(getattr(profile, "id", ""))
            name = (getattr(profile, "display_name", "") or "").strip() or "Bạn"
            row = _menu_row(name, "check" if pid == self._active_id else "", menu)
            row.clicked.connect(lambda _=False, i=pid: self._switch(menu, i))
            box.addWidget(row)

        divider = QFrame(menu)
        divider.setFrameShape(QFrame.Shape.HLine)
        divider.setProperty("class", "hline")
        box.addWidget(divider)

        add = _menu_row("Thêm hồ sơ", "user-plus", menu)
        add.clicked.connect(lambda: self._create(menu))
        box.addWidget(add)

        rename = _menu_row("Đổi tên hiển thị", "pencil", menu)
        rename.clicked.connect(lambda: self._rename(menu))
        box.addWidget(rename)

        settings = _menu_row("Cài đặt", "settings", menu)
        settings.clicked.connect(lambda: (menu.hide(), self.settingsRequested.emit()))
        box.addWidget(settings)

        if len(self._profiles) > 1:
            remove = ConfirmButton(
                "Xóa hồ sơ này",
                "Bấm lại để xóa hồ sơ và bộ nhớ của nó",
                icon_name="trash-2",
                parent=menu,
            )
            remove.confirmed.connect(lambda: self._delete(menu))
            box.addWidget(remove)

        menu.adjustSize()
        menu.move(self.mapToGlobal(QPoint(0, -menu.sizeHint().height() - theme.SPACE["xs"])))
        menu.show()
        self._menu = menu

    def _switch(self, menu: QFrame, profile_id: str) -> None:
        menu.hide()
        if not profile_id or profile_id == self._active_id:
            return
        from private_ai.core import repositories

        def done(_record) -> None:
            self.ctx.toast("Đã chuyển hồ sơ", "success")
            self.profileChanged.emit(profile_id)
            self.refresh()

        self.ctx.run(
            repositories.activate_profile(self.ctx.database, profile_id),
            on_result=done,
            on_error=lambda error: self.ctx.toast(f"Không chuyển được hồ sơ: {error}", "error"),
        )

    def _create(self, menu: QFrame) -> None:
        menu.hide()
        name, ok = QInputDialog.getText(self, "Thêm hồ sơ", "Tên hiển thị")
        if not ok or not name.strip():
            return
        from private_ai.core import repositories

        def done(record) -> None:
            self.ctx.toast("Đã tạo hồ sơ mới", "success")
            self.profileChanged.emit(str(getattr(record, "id", "")))
            self.refresh()

        self.ctx.run(repositories.create_profile(self.ctx.database, name.strip()), on_result=done)

    def _rename(self, menu: QFrame) -> None:
        menu.hide()
        if not self._active_id:
            return
        name, ok = QInputDialog.getText(
            self, "Đổi tên hiển thị", "Tên hiển thị", text=self._active_name
        )
        if not ok or not name.strip():
            return
        from private_ai.core import repositories

        self.ctx.run(
            repositories.rename_profile(self.ctx.database, self._active_id, name.strip()),
            on_result=lambda _: (self.ctx.toast("Đã lưu tên", "success"), self.refresh()),
        )

    def _delete(self, menu: QFrame) -> None:
        menu.hide()
        if not self._active_id:
            return
        from private_ai.core import repositories

        def done(_result) -> None:
            self.ctx.toast("Đã xóa hồ sơ", "success")
            self.refresh()

        self.ctx.run(
            repositories.delete_profile(self.ctx.database, self._active_id, confirmed=True),
            on_result=done,
            on_error=lambda error: self.ctx.toast(f"Không xóa được hồ sơ: {error}", "error"),
        )
