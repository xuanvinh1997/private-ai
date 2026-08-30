"""Where the models actually run: the configured AI providers.

``ProviderRegistry`` is a thin synchronous SQLite view, so listing and activating happen
inline; only the network probe goes through ``ctx.run``.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

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

from private_ai.ui.dialogs.provider_dialog import KIND_LABELS, ProviderDialog, probe_provider
from private_ai.ui.icons import icon
from private_ai.ui.widgets.confirm_button import ConfirmButton

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.llm.registry import ProviderConfig
    from private_ai.ui.context import AppContext


class _ProviderRow(QFrame):
    def __init__(
        self,
        view: ProvidersView,
        provider: ProviderConfig,
        active: bool,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self._provider = provider
        self.setProperty("class", "card")

        layout = QHBoxLayout(self)
        layout.setSpacing(12)

        identity = QVBoxLayout()
        identity.setSpacing(2)
        name = QLabel(provider.name)
        name.setProperty("class", "subtitle")
        identity.addWidget(name)
        endpoint = QLabel(f"{KIND_LABELS.get(provider.kind, provider.kind)} · {provider.base_url}")
        endpoint.setWordWrap(True)
        endpoint.setProperty("class", "muted")
        identity.addWidget(endpoint)
        notes = []
        if provider.api_key:
            notes.append("Đã lưu khóa API")
        if provider.builtin:
            notes.append("Ollama trên máy này")
        if provider.on_device:
            notes.append("chạy trên máy này")
        if notes:
            hint = QLabel(" · ".join(notes))
            hint.setProperty("class", "faint")
            identity.addWidget(hint)
        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "faint")
        self._status.hide()
        identity.addWidget(self._status)
        layout.addLayout(identity, 1)

        state = QLabel("Đang dùng" if active else "Chưa dùng")
        state.setProperty("class", "chip-active" if active else "chip")
        layout.addWidget(state, 0, Qt.AlignmentFlag.AlignTop)

        actions = QHBoxLayout()
        actions.setSpacing(6)
        if not active:
            use = QPushButton("Dùng")
            use.clicked.connect(lambda: view.activate(provider))
            actions.addWidget(use)
        check = QPushButton("Kiểm tra")
        check.clicked.connect(self._on_probe)
        actions.addWidget(check)
        edit = QPushButton("Sửa")
        edit.clicked.connect(lambda: view.edit(provider))
        actions.addWidget(edit)
        remove = ConfirmButton("Xóa", "Xác nhận xóa")
        remove.confirmed.connect(lambda: view.remove(provider))
        actions.addWidget(remove)
        layout.addLayout(actions, 0)

        self._view = view

    def _on_probe(self) -> None:
        self._status.setText("Đang kiểm tra kết nối…")
        self._status.show()
        self._view.ctx.run(
            probe_provider(self._provider.kind, self._provider.base_url, self._provider.api_key),
            on_result=self._show,
            on_error=lambda exc: self._show_text(str(exc) or "Không kiểm tra được kết nối"),
        )

    def _show(self, result) -> None:
        self._show_text(
            f"Kết nối tốt · {result.model_count} mô hình"
            if result.reachable
            else (result.detail or "Không kết nối được")
        )

    def _show_text(self, message: str) -> None:
        self._status.setText(message)
        self._status.show()


class ProvidersView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.ctx = ctx

        root = QVBoxLayout(self)
        root.setContentsMargins(4, 4, 4, 4)
        root.setSpacing(12)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Nguồn suy luận")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Nhà cung cấp AI")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Chọn nơi chạy mô hình: Ollama trên máy hoặc bất kỳ máy chủ nào theo chuẩn "
            "OpenAI API. Trò chuyện, embedding và trích xuất tri thức đều dùng nhà cung cấp "
            "đang bật."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)
        add = QPushButton("Thêm nhà cung cấp")
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

        self.refresh()

    def on_activated(self) -> None:
        self.refresh()

    # --- rendering --------------------------------------------------------

    def refresh(self) -> None:
        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        registry = self.ctx.services.providers
        try:
            configs = registry.list_configs()
            active = registry.active_id()
        except Exception as exc:  # noqa: BLE001 - a broken table must not blank the tab
            self._empty.setText(f"Không đọc được danh sách nhà cung cấp: {exc}")
            self._empty.show()
            self._scroll.hide()
            return
        if not configs:
            self._empty.setText(
                "Chưa có nhà cung cấp nào.\n"
                "Thêm một máy chủ để trò chuyện, tạo embedding và trích xuất tri thức."
            )
            self._empty.show()
            self._scroll.hide()
            return
        self._empty.hide()
        self._scroll.show()
        for config in configs:
            row = _ProviderRow(self, config, config.id == active, self._canvas)
            self._rows.insertWidget(self._rows.count() - 1, row)

    # --- actions ----------------------------------------------------------

    def _on_add(self) -> None:
        dialog = ProviderDialog(self.ctx, None, self)
        dialog.saved.connect(self._changed)
        dialog.exec()

    def edit(self, provider: ProviderConfig) -> None:
        dialog = ProviderDialog(self.ctx, provider, self)
        dialog.saved.connect(self._changed)
        dialog.exec()

    def activate(self, provider: ProviderConfig) -> None:
        try:
            self.ctx.services.providers.activate(provider.id)
        except (ValueError, LookupError) as exc:
            self.ctx.toast(str(exc) or "Không chuyển được nhà cung cấp", "error")
            return
        self.ctx.toast(f"Đang dùng {provider.name}", "success")
        self._changed()

    def remove(self, provider: ProviderConfig) -> None:
        try:
            self.ctx.services.providers.delete(provider.id)
        except LookupError as exc:
            self.ctx.toast(str(exc) or "Không xóa được nhà cung cấp", "error")
            return
        self.ctx.toast(f"Đã xóa {provider.name}", "success")
        self._changed()

    def _changed(self) -> None:
        self.refresh()
        self.ctx.modelsChanged.emit()
