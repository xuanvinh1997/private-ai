"""Installed models: state, size, per-task defaults, and the lifecycle actions."""

from __future__ import annotations

from typing import TYPE_CHECKING

import httpx
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.llm.admin import pull_fraction
from private_ai.ui.dialogs.add_model_dialog import AddModelDialog
from private_ai.ui.format import format_bytes
from private_ai.ui.icons import icon
from private_ai.ui.models.models_model import (
    TASK_LABELS,
    ModelsModel,
    initials_of,
    state_label,
    state_pip,
)
from private_ai.ui.widgets.confirm_button import ConfirmButton
from private_ai.ui.widgets.status_pip import StatusPip

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import ModelInfo
    from private_ai.ui.context import AppContext

LOAD_TIMEOUT_SECONDS = 120.0


class _ModelRow(QFrame):
    def __init__(self, view: ModelsView, model: ModelInfo, parent=None) -> None:
        super().__init__(parent)
        self._view = view
        self._model = model
        self.setProperty("class", "card")

        layout = QHBoxLayout(self)
        layout.setSpacing(12)

        glyph = QLabel(initials_of(model.name))
        glyph.setProperty("class", "code")
        glyph.setFixedWidth(48)
        glyph.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(glyph, 0, Qt.AlignmentFlag.AlignTop)

        identity = QVBoxLayout()
        identity.setSpacing(2)
        name = QLabel(model.name)
        name.setProperty("class", "subtitle")
        identity.addWidget(name)
        traits = " · ".join(model.capabilities) or model.model_type
        summary = QLabel(f"{model.runtime} · {traits}")
        summary.setProperty("class", "muted")
        summary.setWordWrap(True)
        identity.addWidget(summary)
        if model.default_for:
            tasks = ", ".join(TASK_LABELS.get(task, task) for task in model.default_for)
            badge = QLabel(f"Mặc định: {tasks}")
            badge.setProperty("class", "chip-active")
            identity.addWidget(badge, 0, Qt.AlignmentFlag.AlignLeft)
        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "faint")
        self._status.hide()
        identity.addWidget(self._status)
        layout.addLayout(identity, 1)

        metric = QVBoxLayout()
        metric.setSpacing(2)
        metric_label = QLabel("Dung lượng")
        metric_label.setProperty("class", "faint")
        metric.addWidget(metric_label)
        size = QLabel(format_bytes(model.size_bytes))
        size.setProperty("class", "subtitle")
        metric.addWidget(size)
        if model.quantization:
            quantization = QLabel(str(model.quantization))
            quantization.setProperty("class", "faint")
            metric.addWidget(quantization)
        if model.sha256:
            digest = QLabel(f"SHA {model.sha256[:12]}…")
            digest.setProperty("class", "faint")
            metric.addWidget(digest)
        layout.addLayout(metric, 0)

        state = QHBoxLayout()
        state.setSpacing(6)
        state.addWidget(StatusPip(state_pip(model.state)))
        state.addWidget(QLabel(state_label(model.state)))
        layout.addLayout(state, 0)

        actions = QHBoxLayout()
        actions.setSpacing(6)
        self._buttons: list[QPushButton] = []
        if str(model.state) == "unloaded":
            self._add(actions, "Nạp", lambda: view.load(self, self._model))
        if str(model.state) == "loaded":
            self._add(actions, "Dỡ khỏi bộ nhớ", lambda: view.unload(self, self._model))
        if "vision" in model.capabilities and "vision" not in model.default_for:
            self._add(actions, "Dùng cho OCR", lambda: view.use_for_ocr(self, self._model))
        self._add(actions, "Cập nhật", lambda: view.update_model(self, self._model))
        remove = ConfirmButton("Xóa", "Xác nhận xóa")
        remove.confirmed.connect(lambda: view.remove(self, self._model))
        actions.addWidget(remove)
        self._buttons.append(remove)
        layout.addLayout(actions, 0)

    def _add(self, layout: QHBoxLayout, text: str, handler) -> None:
        button = QPushButton(text)
        button.clicked.connect(handler)
        layout.addWidget(button)
        self._buttons.append(button)

    def set_status(self, message: str) -> None:
        self._status.setText(message)
        self._status.setVisible(bool(message))

    def set_working(self, working: bool) -> None:
        for button in self._buttons:
            button.setEnabled(not working)


class ModelsView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._model = ModelsModel(self)
        self._loading = False

        root = QVBoxLayout(self)
        root.setContentsMargins(4, 4, 4, 4)
        root.setSpacing(12)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Mô hình cục bộ")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Quản lý mô hình")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Trạng thái tải, dung lượng, khả năng và mặc định theo tác vụ của từng mô hình."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)
        add = QPushButton("Thêm mô hình")
        add.setIcon(icon("plus"))
        add.setProperty("class", "primary")
        add.clicked.connect(self._on_add)
        heading.addWidget(add, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        root.addWidget(self._empty)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._rows = QVBoxLayout(self._canvas)
        self._rows.setSpacing(8)
        self._rows.setContentsMargins(0, 0, 0, 0)
        self._rows.addStretch(1)
        self._scroll.setWidget(self._canvas)
        root.addWidget(self._scroll, 1)

        ctx.modelsChanged.connect(self.refresh)
        self.refresh()

    def on_activated(self) -> None:
        self.refresh()

    def chat_models(self) -> list[ModelInfo]:
        return self._model.chat_models()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        if self._loading:
            return
        self._loading = True
        self._empty.setText("Đang đọc thư viện mô hình…")
        self._empty.show()
        self._ctx.run(self._load(), on_result=self._loaded, on_error=self._failed)

    async def _load(self) -> tuple[list[ModelInfo], dict[str, str]]:
        models = await self._ctx.services.models.list_models()
        defaults = await repositories.get_model_defaults(self._ctx.database)
        return models, defaults

    def _loaded(self, payload: tuple[list[ModelInfo], dict[str, str]]) -> None:
        self._loading = False
        models, defaults = payload
        self._model.set_records(models, defaults)
        self._render()

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._model.set_records([], {})
        self._empty.setText(
            f"Không đọc được thư viện mô hình.\n{exc}\nKhởi động nhà cung cấp rồi thử lại."
        )
        self._empty.show()
        self._scroll.hide()

    def _render(self) -> None:
        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        records = self._model.records()
        if not records:
            self._empty.setText(
                "Chưa tìm thấy mô hình.\nKhởi động Ollama rồi thêm mô hình đầu tiên."
            )
            self._empty.show()
            self._scroll.hide()
            return
        self._empty.hide()
        self._scroll.show()
        for record in records:
            self._rows.insertWidget(self._rows.count() - 1, _ModelRow(self, record, self._canvas))

    # --- actions ----------------------------------------------------------

    def _on_add(self) -> None:
        dialog = AddModelDialog(self._ctx, self)
        dialog.completed.connect(lambda _: self._changed())
        dialog.exec()

    def _run_row(self, row: _ModelRow, coro, busy: str, failure: str) -> None:
        row.set_working(True)
        row.set_status(busy)
        self._ctx.run(
            coro,
            on_result=lambda _: self._changed(),
            on_error=lambda exc: self._row_failed(row, exc, failure),
        )

    def _row_failed(self, row: _ModelRow, exc: BaseException, failure: str) -> None:
        row.set_working(False)
        row.set_status(str(exc) or failure)

    def _changed(self) -> None:
        self.refresh()
        self._ctx.modelsChanged.emit()

    async def _load_model(self, name: str) -> None:
        """Ollama has no explicit load verb; an empty generate with a keep-alive is it."""
        admin = self._ctx.services.models.admin
        base_url = admin.provider("nạp mô hình").base_url.rstrip("/")
        async with httpx.AsyncClient(timeout=LOAD_TIMEOUT_SECONDS) as client:
            response = await client.post(
                f"{base_url}/api/generate",
                json={"model": name, "keep_alive": "5m"},
            )
            response.raise_for_status()

    def load(self, row: _ModelRow, model: ModelInfo) -> None:
        self._run_row(
            row, self._load_model(model.name), "Đang nạp vào bộ nhớ…", "Không thể nạp mô hình"
        )

    def unload(self, row: _ModelRow, model: ModelInfo) -> None:
        self._run_row(
            row,
            self._ctx.services.models.admin.unload(model.name),
            "Đang dỡ khỏi bộ nhớ…",
            "Không thể dỡ mô hình",
        )

    def use_for_ocr(self, row: _ModelRow, model: ModelInfo) -> None:
        self._run_row(
            row,
            repositories.set_model_default(self._ctx.database, "vision", model.name),
            "Đang đặt làm mô hình OCR…",
            "Không thể chọn mô hình OCR",
        )

    def remove(self, row: _ModelRow, model: ModelInfo) -> None:
        self._run_row(
            row,
            self._delete(model.name),
            "Đang xóa mô hình…",
            "Không thể xóa mô hình",
        )

    async def _delete(self, name: str) -> None:
        admin = self._ctx.services.models.admin
        try:
            await admin.delete(name)
        except Exception as exc:
            await repositories.record_model_event(
                self._ctx.database, name, "delete", "failed", str(exc)
            )
            raise
        await repositories.record_model_event(self._ctx.database, name, "delete", "completed")

    def update_model(self, row: _ModelRow, model: ModelInfo) -> None:
        row.set_working(True)
        row.set_status("Đang kiểm tra bản cập nhật…")
        self._ctx.run(
            self._pull(row, model.name),
            on_result=lambda _: self._changed(),
            on_error=lambda exc: self._row_failed(row, exc, "Không thể cập nhật mô hình"),
        )

    async def _pull(self, row: _ModelRow, name: str) -> None:
        admin = self._ctx.services.models.admin
        try:
            async for event in admin.pull(name):
                status = str(event.get("status") or "").strip()
                fraction = pull_fraction(event)
                row.set_status(
                    f"{status} · {round(fraction * 100)}%" if fraction else status or "Đang tải…"
                )
                if str(event.get("error") or "").strip():
                    raise RuntimeError(str(event["error"]))
        except Exception as exc:
            await repositories.record_model_event(
                self._ctx.database, name, "pull", "failed", str(exc)
            )
            raise
        await repositories.record_model_event(self._ctx.database, name, "pull", "completed")
