"""What Private AI remembers, and the controls to change it."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import TYPE_CHECKING

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui.icons import icon
from private_ai.ui.models.memory_model import TYPE_LABELS, MemoryModel, type_label
from private_ai.ui.theme import (
    CARD_MARGINS,
    CARD_SPACING,
    DIALOG_MARGINS,
    DIALOG_SPACING,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
)
from private_ai.ui.widgets.confirm_button import ConfirmButton

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import MemoryRecord
    from private_ai.ui.context import AppContext

# Enough for a few sentences of a remembered note before the editor starts scrolling.
_CONTENT_HEIGHT = SPACE["3xl"] * 3 + SPACE["2xl"]


class MemoryDialog(QDialog):
    """Add or edit one memory. Type and content only — the rest is bookkeeping."""

    def __init__(self, record: MemoryRecord | None = None, parent=None) -> None:
        super().__init__(parent)
        self.setModal(True)
        self.setWindowTitle("Sửa bộ nhớ" if record else "Thêm bộ nhớ")
        self.setMinimumWidth(460)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(*DIALOG_MARGINS)
        layout.setSpacing(DIALOG_SPACING)

        heading = QLabel(self.windowTitle())
        heading.setProperty("class", "title")
        layout.addWidget(heading)
        blurb = QLabel("Chỉ lưu những điều bạn chủ động nhập tại đây.")
        blurb.setProperty("class", "muted")
        blurb.setWordWrap(True)
        layout.addWidget(blurb)

        layout.addWidget(QLabel("Loại"))
        self._type = QComboBox()
        for value, label in TYPE_LABELS.items():
            self._type.addItem(label, value)
        if record is not None:
            index = self._type.findData(str(record.type))
            if index >= 0:
                self._type.setCurrentIndex(index)
        layout.addWidget(self._type)

        layout.addWidget(QLabel("Nội dung"))
        self._content = QPlainTextEdit(record.content if record else "")
        self._content.setFixedHeight(_CONTENT_HEIGHT)
        layout.addWidget(self._content)

        row = QHBoxLayout()
        row.setSpacing(TOOLBAR_SPACING)
        row.addStretch(1)
        cancel = QPushButton("Hủy")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        save = QPushButton("Lưu")
        save.setProperty("class", "primary")
        save.setDefault(True)
        save.clicked.connect(self._on_save)
        row.addWidget(save)
        layout.addLayout(row)

        self._content.setFocus(Qt.FocusReason.OtherFocusReason)

    def _on_save(self) -> None:
        if self._content.toPlainText().strip():
            self.accept()

    def values(self) -> tuple[str, str]:
        return str(self._type.currentData()), self._content.toPlainText().strip()


class _MemoryRow(QFrame):
    def __init__(self, view: MemoryView, record: MemoryRecord, parent=None) -> None:
        super().__init__(parent)
        self.setProperty("class", "card")
        self.setEnabled(True)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(*CARD_MARGINS)
        layout.setSpacing(CARD_SPACING)

        # A memory type is a category, not a state, so it takes the neutral badge.
        kind = QLabel(type_label(record.type))
        kind.setProperty("class", "chip")
        layout.addWidget(kind, 0, Qt.AlignmentFlag.AlignTop)

        content = QLabel(record.content)
        content.setWordWrap(True)
        # A disabled memory is still the text the user came here to read.
        content.setProperty("class", "" if record.enabled else "muted")
        layout.addWidget(content, 1)

        actions = QHBoxLayout()
        actions.setSpacing(TOOLBAR_SPACING)
        edit = QPushButton("Sửa")
        edit.clicked.connect(lambda: view.edit(record))
        actions.addWidget(edit)
        toggle = QPushButton("Tắt" if record.enabled else "Bật")
        toggle.clicked.connect(lambda: view.set_enabled(record, not record.enabled))
        actions.addWidget(toggle)
        remove = ConfirmButton("Xóa", "Xác nhận xóa")
        remove.confirmed.connect(lambda: view.remove(record))
        actions.addWidget(remove)
        layout.addLayout(actions, 0)
        # The content column wraps, so the two side columns hang from the same top edge.
        layout.setAlignment(actions, Qt.AlignmentFlag.AlignTop)


class MemoryView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._model = MemoryModel(self)
        self._profile_id = ""
        self._loading = False

        root = QVBoxLayout(self)
        # Hosted inside the settings tab widget, which already supplies the page
        # padding; a second PAGE_MARGINS here would inset the tab twice.
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(PAGE_SPACING)

        heading = QHBoxLayout()
        heading.setSpacing(TOOLBAR_SPACING)
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["2xs"])
        eyebrow = QLabel("Bộ nhớ cá nhân")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Điều Private AI ghi nhớ")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel("Bạn kiểm soát từng mục đã lưu và có thể tắt hoặc xóa bất cứ lúc nào.")
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)

        self._export = QPushButton("Xuất JSON")
        self._export.setIcon(icon("download"))
        self._export.clicked.connect(self._on_export)
        heading.addWidget(self._export, 0, Qt.AlignmentFlag.AlignTop)
        add = QPushButton("Thêm bộ nhớ")
        add.setIcon(icon("plus"))
        add.setProperty("class", "primary")
        add.clicked.connect(self._on_add)
        heading.addWidget(add, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        # Stretch lives here as well as on the scroll area: the empty state hides the
        # scroll, and a column with no expanding child hands the surplus to the page
        # header instead, which stretches the title to five times its own height.
        root.addWidget(self._empty, 1)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._rows = QVBoxLayout(self._canvas)
        self._rows.setSpacing(SPACE["sm"])
        self._rows.setContentsMargins(0, 0, 0, 0)
        self._rows.addStretch(1)
        self._scroll.setWidget(self._canvas)
        root.addWidget(self._scroll, 1)

        self.refresh()

    # --- lifecycle --------------------------------------------------------

    def on_activated(self) -> None:
        self.refresh()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        if self._loading:
            return
        self._loading = True
        self._ctx.run(self._load(), on_result=self._loaded, on_error=self._failed)

    async def _load(self) -> list[MemoryRecord]:
        database = self._ctx.database
        # Keyed on the profile so switching accounts reloads the list instead of showing
        # the previous person's memories.
        self._profile_id = await repositories.active_profile_id_async(database)
        return await repositories.list_memories(database, self._profile_id, include_disabled=True)

    def _loaded(self, records: list[MemoryRecord]) -> None:
        self._loading = False
        self._model.set_records(records)
        self._render()

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._ctx.toast(str(exc) or "Không đọc được bộ nhớ", "error")

    # --- rendering --------------------------------------------------------

    def _render(self) -> None:
        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        records = self._model.records()
        self._export.setEnabled(bool(records))
        if not records:
            self._empty.setText(
                "Chưa có thông tin nào được lưu.\nThêm sở thích hoặc thông tin bạn muốn AI ghi nhớ."
            )
            self._empty.show()
            self._scroll.hide()
            return
        self._empty.hide()
        self._scroll.show()
        for record in records:
            self._rows.insertWidget(self._rows.count() - 1, _MemoryRow(self, record))

    # --- actions ----------------------------------------------------------

    def _on_add(self) -> None:
        dialog = MemoryDialog(None, self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        memory_type, content = dialog.values()
        self._ctx.run(
            self._ctx.services.memory.remember(
                content,
                memory_type=memory_type,
                source="user",
                user_id=self._profile_id,
            ),
            on_result=lambda _: self._saved("Đã lưu bộ nhớ"),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không thể lưu bộ nhớ", "error"),
        )

    def edit(self, record: MemoryRecord) -> None:
        dialog = MemoryDialog(record, self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        memory_type, content = dialog.values()
        self._ctx.run(
            self._apply_edit(record, memory_type, content),
            on_result=lambda _: self._saved("Đã lưu bộ nhớ"),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không thể lưu bộ nhớ", "error"),
        )

    async def _apply_edit(self, record: MemoryRecord, memory_type: str, content: str) -> None:
        if memory_type != str(record.type):
            # ``MemoryStore.update`` owns the text and the vector, not the classification.
            await self._ctx.database.execute_async(
                "UPDATE memories SET type = ? WHERE id = ?",
                (memory_type, record.id),
            )
        await self._ctx.services.memory.update(record.id, content, record.enabled)

    def set_enabled(self, record: MemoryRecord, enabled: bool) -> None:
        self._ctx.run(
            self._ctx.services.memory.update(record.id, record.content, enabled),
            on_result=lambda _: self._saved("Đã bật bộ nhớ" if enabled else "Đã tắt bộ nhớ"),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không đổi được bộ nhớ", "error"),
        )

    def remove(self, record: MemoryRecord) -> None:
        self._ctx.run(
            self._ctx.services.memory.forget(record.id, confirmed=True),
            on_result=lambda _: self._saved("Đã xóa bộ nhớ"),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không thể xóa bộ nhớ", "error"),
        )

    def _saved(self, message: str) -> None:
        self._ctx.toast(message, "success")
        self.refresh()

    def _on_export(self) -> None:
        stamp = datetime.now(UTC).date().isoformat()
        path, _ = QFileDialog.getSaveFileName(
            self,
            "Xuất bộ nhớ ra JSON",
            f"private-ai-memories-{stamp}.json",
            "JSON (*.json)",
        )
        if not path:
            return
        payload = {
            "exported_at": datetime.now(UTC).isoformat(),
            "memories": self._model.export_payload(),
        }
        try:
            with open(path, "w", encoding="utf-8") as handle:
                json.dump(payload, handle, ensure_ascii=False, indent=2)
        except OSError as exc:
            self._ctx.toast(f"Không ghi được tệp: {exc}", "error")
            return
        self._ctx.toast(f"Đã xuất {len(payload['memories'])} mục", "success")
