"""The workspace card grid."""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QFrame,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QScrollArea,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui.dialogs.workspace_dialog import WorkspaceDialog
from private_ai.ui.format import format_relative_time
from private_ai.ui.icons import icon
from private_ai.ui.models.workspaces_model import WorkspaceFilterProxy, WorkspacesModel
from private_ai.ui.widgets.confirm_button import ConfirmButton

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import WorkspaceRecord
    from private_ai.ui.context import AppContext

COLUMNS = 3


class _WorkspaceCard(QFrame):
    """One card. The whole surface opens the workspace; only the two controls opt out,
    so there is no dead space where a click looks ignored."""

    opened = Signal(str)
    editRequested = Signal(object)
    deleteRequested = Signal(object)

    def __init__(self, record: WorkspaceRecord, active: bool, parent=None) -> None:
        super().__init__(parent)
        self._record = record
        self.setProperty("class", "card")
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setMinimumWidth(240)

        layout = QVBoxLayout(self)
        layout.setSpacing(8)

        top = QHBoxLayout()
        name = QLabel(record.name)
        name.setProperty("class", "subtitle")
        name.setWordWrap(True)
        top.addWidget(name, 1)
        if active:
            badge = QLabel("Đang dùng")
            badge.setProperty("class", "chip-active")
            top.addWidget(badge, 0, Qt.AlignmentFlag.AlignTop)
        layout.addLayout(top)

        identity = QLabel(record.id[:8])
        identity.setProperty("class", "faint")
        layout.addWidget(identity)

        description = QLabel(record.description or "Chưa có mô tả")
        description.setWordWrap(True)
        description.setProperty("class", "muted" if not record.description else "")
        layout.addWidget(description, 1)

        meta = QLabel(
            f"{record.conversation_count} cuộc trò chuyện · "
            f"cập nhật {format_relative_time(record.updated_at)}"
        )
        meta.setProperty("class", "faint")
        layout.addWidget(meta)

        actions = QHBoxLayout()
        actions.addStretch(1)
        edit = QToolButton()
        edit.setIcon(icon("pencil"))
        edit.setToolTip(f"Sửa {record.name}")
        edit.clicked.connect(lambda: self.editRequested.emit(self._record))
        actions.addWidget(edit)
        remove = ConfirmButton("Xóa", "Xóa hẳn", icon_name="trash-2")
        remove.confirmed.connect(lambda: self.deleteRequested.emit(self._record))
        actions.addWidget(remove)
        layout.addLayout(actions)

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802
        if event.button() == Qt.MouseButton.LeftButton:
            self.opened.emit(self._record.id)
        super().mouseReleaseEvent(event)


class WorkspacesView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._model = WorkspacesModel(self)
        self._proxy = WorkspaceFilterProxy(self)
        self._proxy.setSourceModel(self._model)
        self._loading = False

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 20, 24, 20)
        root.setSpacing(14)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Không gian làm việc")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Quản lý không gian")
        title.setProperty("class", "title")
        titles.addWidget(title)
        self._stats = QLabel("")
        self._stats.setProperty("class", "muted")
        titles.addWidget(self._stats)
        heading.addLayout(titles, 1)

        create = QPushButton("Tạo không gian")
        create.setIcon(icon("plus"))
        create.setProperty("class", "primary")
        create.clicked.connect(self._on_create)
        heading.addWidget(create, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        self._search = QLineEdit()
        self._search.setClearButtonEnabled(True)
        self._search.setPlaceholderText("Tìm theo tên hoặc mô tả")
        self._search.addAction(icon("search"), QLineEdit.ActionPosition.LeadingPosition)
        self._search.textChanged.connect(self._on_search)
        root.addWidget(self._search)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        root.addWidget(self._empty)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._grid = QGridLayout(self._canvas)
        self._grid.setSpacing(12)
        self._grid.setContentsMargins(0, 0, 0, 0)
        self._scroll.setWidget(self._canvas)
        root.addWidget(self._scroll, 1)

        ctx.workspaceChanged.connect(lambda _: self._render())
        self.refresh()

    # --- lifecycle --------------------------------------------------------

    def on_activated(self) -> None:
        self.refresh()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        if self._loading:
            return
        self._loading = True
        self._ctx.run(
            repositories.list_workspaces(self._ctx.database),
            on_result=self._loaded,
            on_error=self._failed,
        )

    def _loaded(self, records: list[WorkspaceRecord]) -> None:
        self._loading = False
        self._model.set_records(records)
        self._render()

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._ctx.toast(str(exc) or "Không đọc được danh sách không gian", "error")

    def _on_search(self, text: str) -> None:
        self._proxy.set_term(text)
        self._render()

    # --- rendering --------------------------------------------------------

    def _clear_grid(self) -> None:
        while self._grid.count():
            item = self._grid.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

    def _render(self) -> None:
        self._clear_grid()
        total = self._model.rowCount()
        if total:
            self._stats.setText(
                f"{total} không gian · {self._model.total_conversations()} cuộc trò chuyện"
            )
        else:
            self._stats.setText("Tạo không gian đầu tiên để nhóm các cuộc trò chuyện và tài liệu.")
        self._search.setVisible(total > 0)

        visible = self._proxy.rowCount()
        if not total:
            self._empty.setText(
                "Chưa có không gian làm việc.\n"
                "Mỗi không gian giữ riêng cuộc trò chuyện và tài liệu của một dự án."
            )
            self._empty.show()
            self._scroll.hide()
            return
        if not visible:
            self._empty.setText("Không có không gian nào khớp. Thử từ khóa khác.")
            self._empty.show()
            self._scroll.hide()
            return
        self._empty.hide()
        self._scroll.show()

        active = self._ctx.workspace_id
        for position in range(visible):
            record = self._proxy.record_at(position)
            if record is None:
                continue
            card = _WorkspaceCard(record, record.id == active, self._canvas)
            card.opened.connect(self._open)
            card.editRequested.connect(self._on_edit)
            card.deleteRequested.connect(self._on_delete)
            self._grid.addWidget(card, position // COLUMNS, position % COLUMNS)
        self._grid.setRowStretch(self._grid.rowCount(), 1)

    # --- actions ----------------------------------------------------------

    def _open(self, workspace_id: str) -> None:
        self._ctx.set_workspace(workspace_id)
        self._ctx.navigate("chat")

    def _on_create(self) -> None:
        dialog = WorkspaceDialog(self._ctx, None, self)
        dialog.saved.connect(self._after_save)
        dialog.exec()

    def _on_edit(self, record: WorkspaceRecord) -> None:
        dialog = WorkspaceDialog(self._ctx, record, self)
        dialog.saved.connect(self._after_save)
        dialog.exec()

    def _after_save(self, record: WorkspaceRecord, created: bool) -> None:
        self._ctx.toast(f"Đã tạo {record.name}" if created else f"Đã lưu {record.name}", "success")
        if created:
            self._ctx.set_workspace(record.id)
        self.refresh()

    def _on_delete(self, record: WorkspaceRecord) -> None:
        self._ctx.run(
            repositories.delete_workspace(self._ctx.database, record.id, confirmed=True),
            on_result=lambda documents: self._deleted(record, documents),
            on_error=lambda exc: self._ctx.toast(
                str(exc) or "Không thể xóa không gian làm việc", "error"
            ),
        )

    def _deleted(self, record: WorkspaceRecord, documents: list[str]) -> None:
        # The rows cascade, but the files and graph nodes behind those ids do not; the
        # ingestion pipeline owns both, so it finishes the job.
        for document_id in documents:
            self._ctx.run(
                self._ctx.services.ingestion.delete_document(document_id, confirmed=True),
                on_error=lambda _: None,
            )
        if self._ctx.workspace_id == record.id:
            self._ctx.set_workspace("")
        self._ctx.toast(f"Đã xóa {record.name}", "success")
        self._ctx.documentsChanged.emit()
        self.refresh()
