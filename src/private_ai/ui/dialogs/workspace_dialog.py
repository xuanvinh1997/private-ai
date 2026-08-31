"""Create or edit one workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QDialog,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QPushButton,
)

from private_ai.core import repositories
from private_ai.core.schemas import WorkspaceCreate, WorkspaceUpdate
from private_ai.ui.dialogs import _shell
from private_ai.ui.theme import CONTROL_HEIGHT

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import WorkspaceRecord
    from private_ai.ui.context import AppContext


class WorkspaceDialog(QDialog):
    """Modal for both create and edit; delete is offered only when editing."""

    saved = Signal(object, bool)  # (WorkspaceRecord, created)
    deleted = Signal(str)

    def __init__(
        self,
        ctx: AppContext,
        workspace: WorkspaceRecord | None = None,
        parent=None,
        *,
        allow_delete: bool = False,
    ) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._workspace = workspace
        self._confirm_delete = False
        self._busy = False

        self.setModal(True)
        self.setWindowTitle("Sửa không gian làm việc" if workspace else "Không gian làm việc mới")
        self.setMinimumWidth(460)

        layout = _shell.dialog_layout(self)
        _shell.title_block(
            layout,
            self.windowTitle(),
            "Nhóm các cuộc trò chuyện cùng dự án để tìm lại dễ dàng hơn.",
        )

        self._name = QLineEdit(workspace.name if workspace else "")
        self._name.setMaxLength(120)
        self._name.setPlaceholderText("Ví dụ: Hồ sơ dự án")
        _shell.field(layout, "Tên", self._name)

        self._description = QPlainTextEdit(workspace.description if workspace else "")
        # Three input rows tall: enough for a real description, still on the control grid.
        self._description.setFixedHeight(CONTROL_HEIGHT * 3)
        _shell.field(layout, "Mô tả", self._description)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        # Plain buttons rather than a QDialogButtonBox: the box reorders itself per
        # platform, which is the one thing these six dialogs must not disagree about.
        row = _shell.action_row(layout)
        self._delete = QPushButton("Xóa")
        self._delete.setProperty("class", "danger")
        self._delete.clicked.connect(self._on_delete)
        self._delete.setVisible(bool(workspace) and allow_delete)
        row.addWidget(self._delete)
        row.addStretch(1)
        cancel = QPushButton("Hủy")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        self._save = QPushButton("Lưu")
        self._save.setProperty("class", "primary")
        self._save.setDefault(True)
        self._save.clicked.connect(self._on_save)
        row.addWidget(self._save)

        self._name.setFocus(Qt.FocusReason.OtherFocusReason)
        self._name.returnPressed.connect(self._on_save)

    # --- helpers ----------------------------------------------------------

    def _fail(self, message: str) -> None:
        self._error.setText(message)
        self._error.show()
        self._set_busy(False)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self._save.setEnabled(not busy)
        self._save.setText("Đang lưu…" if busy else "Lưu")

    # --- actions ----------------------------------------------------------

    def _on_save(self) -> None:
        if self._busy:
            return
        name = self._name.text().strip()
        if not name:
            self._fail("Tên không gian không được để trống.")
            return
        description = self._description.toPlainText().strip()
        self._error.hide()
        self._set_busy(True)

        database = self._ctx.database
        if self._workspace is None:
            coro = repositories.create_workspace(
                database, WorkspaceCreate(name=name, description=description)
            )
            created = True
        else:
            coro = repositories.update_workspace(
                database,
                self._workspace.id,
                WorkspaceUpdate(name=name, description=description),
            )
            created = False

        self._ctx.run(
            coro,
            on_result=lambda record: self._done(record, created),
            on_error=lambda exc: self._fail(str(exc) or "Không thể lưu không gian làm việc"),
        )

    def _done(self, record: WorkspaceRecord, created: bool) -> None:
        self._set_busy(False)
        self.saved.emit(record, created)
        self.accept()

    def _on_delete(self) -> None:
        if self._workspace is None or self._busy:
            return
        if not self._confirm_delete:
            self._confirm_delete = True
            self._delete.setText("Bấm lần nữa để xóa")
            return
        workspace_id = self._workspace.id
        self._set_busy(True)
        self._ctx.run(
            repositories.delete_workspace(self._ctx.database, workspace_id, confirmed=True),
            on_result=lambda _: self._deleted(workspace_id),
            on_error=lambda exc: self._fail(str(exc) or "Không thể xóa không gian làm việc"),
        )

    def _deleted(self, workspace_id: str) -> None:
        self._set_busy(False)
        self.deleted.emit(workspace_id)
        self.accept()
