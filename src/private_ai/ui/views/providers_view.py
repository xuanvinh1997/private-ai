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
from private_ai.ui.theme import (
    BADGE_HEIGHT,
    CARD_MARGINS,
    CARD_SPACING,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
    token,
)
from private_ai.ui.widgets.confirm_button import ConfirmToolButton

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
        layout.setContentsMargins(*CARD_MARGINS)
        layout.setSpacing(CARD_SPACING)

        identity = QVBoxLayout()
        identity.setSpacing(SPACE["3xs"])

        title = QHBoxLayout()
        title.setSpacing(SPACE["sm"])
        name = QLabel(provider.name)
        name.setProperty("class", "card-title")
        name.setMinimumHeight(BADGE_HEIGHT)
        title.addWidget(name)
        if active:
            # Only the current selection is marked. "Chưa dùng" on every other row was a
            # label for the absence of a state.
            badge = QLabel("Đang dùng")
            badge.setProperty("class", "chip-active")
            badge.setToolTip("Nhà cung cấp đang được dùng")
            title.addWidget(badge, 0, Qt.AlignmentFlag.AlignVCenter)
        title.addStretch(1)
        identity.addLayout(title)

        # The kind is only worth a line when it is not already the name: a provider called
        # "Ollama" of kind Ollama at an Ollama URL said the same word three times.
        kind_label = KIND_LABELS.get(provider.kind, provider.kind)
        endpoint = QLabel(
            provider.base_url
            if kind_label == provider.name
            else f"{kind_label} · {provider.base_url}"
        )
        endpoint.setWordWrap(True)
        endpoint.setProperty("class", "muted")
        identity.addWidget(endpoint)

        notes = []
        if provider.on_device:
            notes.append("Chạy trên máy này")
        elif provider.builtin:
            notes.append("Cài sẵn")
        if provider.api_key:
            notes.append("Đã lưu khóa API")
        if notes:
            hint = QLabel(" · ".join(notes))
            hint.setProperty("class", "faint")
            identity.addWidget(hint)
        # The probe verdict is the reason this row exists; it never gets the faint step.
        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "muted")
        self._status.hide()
        identity.addWidget(self._status)
        layout.addLayout(identity, 1)

        actions = QHBoxLayout()
        actions.setSpacing(SPACE["3xs"])
        if not active:
            self._add(actions, "check", "Dùng nhà cung cấp này", lambda: view.activate(provider))
        self._add(actions, "zap", "Kiểm tra kết nối", self._on_probe)
        self._add(actions, "pencil", f"Sửa {provider.name}", lambda: view.edit(provider))
        remove = ConfirmToolButton(
            tooltip=f"Xóa {provider.name}",
            confirm_tooltip=f"Bấm lại để xóa hẳn {provider.name}",
        )
        remove.confirmed.connect(lambda: view.remove(provider))
        actions.addWidget(remove)
        layout.addLayout(actions, 0)
        # The identity column wraps, so every column of this row hangs from the top.
        layout.setAlignment(actions, Qt.AlignmentFlag.AlignTop)

        self._view = view

    def _add(self, layout: QHBoxLayout, icon_name: str, tooltip: str, handler) -> None:
        """An action as its verb-icon, the same shape the model rows use."""
        button = QPushButton()
        button.setProperty("class", "icon")
        button.setIcon(icon(icon_name, color=token("muted"), size=SPACE["lg"] - 2))
        button.setToolTip(tooltip)
        button.setAccessibleName(tooltip)
        button.clicked.connect(handler)
        layout.addWidget(button)

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
        # Hosted inside the settings tab widget, which already supplies the page
        # padding; a second PAGE_MARGINS here would inset the tab twice.
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(PAGE_SPACING)

        heading = QHBoxLayout()
        heading.setSpacing(TOOLBAR_SPACING)
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["2xs"])
        eyebrow = QLabel("Nguồn suy luận")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Nhà cung cấp AI")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel("Chọn nơi chạy mô hình: Ollama trên máy, hoặc máy chủ chuẩn OpenAI API.")
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
            # What happened and what to do; the Python message is detail for whoever goes
            # looking, so it lives in the tooltip rather than in the sentence.
            self._empty.setText("Không đọc được danh sách nhà cung cấp.\nKhởi động lại ứng dụng.")
            self._empty.setToolTip(str(exc))
            self._empty.show()
            self._scroll.hide()
            return
        if not configs:
            self._empty.setText(
                "Chưa có nhà cung cấp nào.\n"
                "Thêm một máy chủ để trò chuyện, tạo embedding và trích xuất tri thức."
            )
            self._empty.setToolTip("")
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
