"""The sidebar footer: who you are, and how to become someone else.

Profiles are local identities that own separate memory but share documents and
workspaces; the copy says so, because "add profile" otherwise reads like an account.
Ported from ``apps/web/src/components/Profiles.tsx``.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import QPoint, Qt, Signal
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

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._button = QPushButton(self)
        self._button.setMinimumHeight(52)
        self._button.setCursor(Qt.CursorShape.PointingHandCursor)
        self._button.setStyleSheet(
            f"QPushButton {{ border: 1px solid {theme.token('line')}; border-radius: 11px; "
            f"text-align: left; padding: 0; background: {theme.token('surface')}; }}"
            f"QPushButton:hover {{ background: {theme.token('surface-hover')}; }}"
        )
        self._button.clicked.connect(self._open_menu)
        layout.addWidget(self._button, 1)

        inner = QHBoxLayout(self._button)
        inner.setContentsMargins(9, 7, 10, 7)
        inner.setSpacing(10)
        self._avatar = QLabel(self._button)
        self._avatar.setFixedSize(32, 32)
        self._avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._avatar.setStyleSheet(
            f"background: {theme.token('accent-soft')}; color: {theme.token('accent-ink')}; "
            f'border-radius: 9px; font-family: "IBM Plex Mono", monospace; font-weight: 700;'
        )
        inner.addWidget(self._avatar)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(1)
        self._name = QLabel(self._active_name, self._button)
        self._name.setStyleSheet(f"color: {theme.token('ink')}; font-weight: 700;")
        status = QHBoxLayout()
        status.setContentsMargins(0, 0, 0, 0)
        status.setSpacing(6)
        self._pip = StatusPip("online", self._button)
        self._where = QLabel("Trên thiết bị", self._button)
        self._where.setProperty("class", "faint")
        status.addWidget(self._pip)
        status.addWidget(self._where, 1)
        copy.addWidget(self._name)
        copy.addLayout(status)
        inner.addLayout(copy, 1)

        caret = QLabel(self._button)
        caret.setPixmap(icons.pixmap("chevrons-up-down", 15, theme.token("faint")))
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
        box.setContentsMargins(9, 9, 9, 9)
        box.setSpacing(3)

        heading = QLabel("Hồ sơ trên máy này", menu)
        heading.setProperty("class", "section-label")
        box.addWidget(heading)

        for profile in self._profiles:
            pid = str(getattr(profile, "id", ""))
            name = (getattr(profile, "display_name", "") or "").strip() or "Bạn"
            row = QPushButton(f"  {name}", menu)
            row.setProperty("class", "ghost")
            row.setMinimumHeight(34)
            row.setIcon(icons.icon("check", size=15) if pid == self._active_id else icons.icon(""))
            row.clicked.connect(lambda _=False, i=pid: self._switch(menu, i))
            box.addWidget(row)

        divider = QFrame(menu)
        divider.setFrameShape(QFrame.Shape.HLine)
        divider.setProperty("class", "hline")
        box.addWidget(divider)

        add = QPushButton("  Thêm hồ sơ", menu)
        add.setProperty("class", "ghost")
        add.setIcon(icons.icon("user-plus", size=15))
        add.setMinimumHeight(32)
        add.clicked.connect(lambda: self._create(menu))
        box.addWidget(add)

        rename = QPushButton("  Đổi tên hiển thị", menu)
        rename.setProperty("class", "ghost")
        rename.setIcon(icons.icon("pencil", size=15))
        rename.setMinimumHeight(32)
        rename.clicked.connect(lambda: self._rename(menu))
        box.addWidget(rename)

        settings = QPushButton("  Cài đặt", menu)
        settings.setProperty("class", "ghost")
        settings.setIcon(icons.icon("settings", size=15))
        settings.setMinimumHeight(32)
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
        menu.move(self.mapToGlobal(QPoint(0, -menu.sizeHint().height() - 6)))
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
