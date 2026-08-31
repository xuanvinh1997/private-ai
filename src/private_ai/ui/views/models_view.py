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
from private_ai.ui.icons import icon, pixmap
from private_ai.ui.models.models_model import (
    TASK_LABELS,
    ModelsModel,
    initials_of,
    state_label,
    state_pip,
)
from private_ai.ui.theme import (
    CARD_MARGINS,
    CARD_SPACING,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
    token,
)
from private_ai.ui.widgets.confirm_button import ConfirmToolButton
from private_ai.ui.widgets.status_pip import StatusPip

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.schemas import ModelInfo
    from private_ai.ui.context import AppContext

LOAD_TIMEOUT_SECONDS = 120.0

# What a model can do, as one glyph each. Spelled out — "ollama · chat · vision · tools ·
# thinking" — the same five words repeated down every row and read as a paragraph of
# boilerplate; the tooltip still carries the word for anyone who needs it.
CAPABILITY_GLYPHS: dict[str, tuple[str, str]] = {
    "chat": ("message-square-text", "Trò chuyện"),
    "vision": ("eye", "Đọc ảnh"),
    "tools": ("wrench", "Gọi công cụ"),
    "thinking": ("brain", "Suy luận"),
    "embedding": ("waypoints", "Embedding"),
}

# Fixed columns, so size and state line up down the list instead of drifting with the
# width of each model's name.
_METRIC_WIDTH = SPACE["4xl"] * 2 + SPACE["lg"]
_STATE_WIDTH = SPACE["4xl"] * 2 + SPACE["md"]
_CAP_ICON = SPACE["lg"]


def glyph_label(name: str, tooltip: str, *, tone: str = "muted", size: int = _CAP_ICON) -> QLabel:
    """One themed icon as a label. Carries its meaning in the tooltip, not in text beside it."""
    label = QLabel()
    label.setPixmap(pixmap(name, size=size, color=token(tone)))
    label.setFixedSize(size, size)
    label.setToolTip(tooltip)
    label.setAccessibleName(tooltip)
    return label


class _ModelRow(QFrame):
    def __init__(self, view: ModelsView, model: ModelInfo, parent=None) -> None:
        super().__init__(parent)
        self._view = view
        self._model = model
        self.setProperty("class", "card")
        if model.sha256:
            # The digest was a line of the row; nobody reads it, but it has to stay reachable.
            self.setToolTip(f"SHA256 {model.sha256}")

        layout = QHBoxLayout(self)
        layout.setContentsMargins(*CARD_MARGINS)
        layout.setSpacing(CARD_SPACING)

        avatar = QLabel(initials_of(model.name))
        avatar.setProperty("class", "avatar-lg")
        avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        # The identity column wraps, so every column in the row hangs from the top edge.
        layout.addWidget(avatar, 0, Qt.AlignmentFlag.AlignTop)

        identity = QVBoxLayout()
        identity.setSpacing(SPACE["2xs"])

        title = QHBoxLayout()
        title.setSpacing(SPACE["sm"])
        # The name is the row's own title, not a caption — it was rendering muted and small.
        name = QLabel(model.name)
        name.setProperty("class", "card-title")
        title.addWidget(name)
        for task in model.default_for:
            label = TASK_LABELS.get(task, task)
            badge = QLabel(label)
            badge.setProperty("class", "chip-active")
            badge.setToolTip(f"Mặc định cho {label}")
            title.addWidget(badge)
        title.addStretch(1)
        identity.addLayout(title)

        traits = QHBoxLayout()
        traits.setSpacing(SPACE["xs"])
        runtime = QLabel(model.runtime)
        runtime.setProperty("class", "faint")
        traits.addWidget(runtime)
        capabilities = list(model.capabilities) or [model.model_type]
        for capability in capabilities:
            glyph = CAPABILITY_GLYPHS.get(capability)
            if glyph is None:
                unknown = QLabel(capability)
                unknown.setProperty("class", "faint")
                traits.addWidget(unknown)
                continue
            traits.addWidget(glyph_label(glyph[0], glyph[1]))
        traits.addStretch(1)
        identity.addLayout(traits)

        # Pull progress and failure text both land here, so it is not tertiary.
        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "muted")
        self._status.hide()
        identity.addWidget(self._status)
        layout.addLayout(identity, 1)

        metric = QVBoxLayout()
        metric.setSpacing(SPACE["3xs"])
        size_row = QHBoxLayout()
        size_row.setSpacing(SPACE["xs"])
        size_row.addStretch(1)
        size_row.addWidget(glyph_label("hard-drive", "Dung lượng", size=SPACE["md"] + 2))
        size = QLabel(format_bytes(model.size_bytes))
        size.setProperty("class", "body-strong")
        size.setToolTip("Dung lượng")
        size_row.addWidget(size)
        metric.addLayout(size_row)
        if model.quantization:
            quantization = QLabel(str(model.quantization))
            quantization.setProperty("class", "faint")
            quantization.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            quantization.setToolTip("Lượng tử hóa")
            metric.addWidget(quantization)
        metric_box = QWidget()
        metric_box.setLayout(metric)
        metric_box.setFixedWidth(_METRIC_WIDTH)
        metric.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(metric_box, 0, Qt.AlignmentFlag.AlignTop)

        state = QHBoxLayout()
        state.setSpacing(SPACE["xs"])
        state.setContentsMargins(0, 0, 0, 0)
        state.addWidget(StatusPip(state_pip(model.state)))
        state_text = QLabel(state_label(model.state))
        state_text.setProperty("class", "muted")
        state.addWidget(state_text)
        state.addStretch(1)
        state_box = QWidget()
        state_box.setLayout(state)
        state_box.setFixedWidth(_STATE_WIDTH)
        layout.addWidget(state_box, 0, Qt.AlignmentFlag.AlignTop)

        actions = QHBoxLayout()
        actions.setSpacing(SPACE["3xs"])
        self._buttons: list[QPushButton] = []
        if str(model.state) == "unloaded":
            self._add(
                actions, "play", "Nạp vào bộ nhớ", lambda: view.load(self, self._model), "accent"
            )
        if str(model.state) == "loaded":
            self._add(
                actions, "stop-circle", "Dỡ khỏi bộ nhớ", lambda: view.unload(self, self._model)
            )
        if "vision" in model.capabilities and "vision" not in model.default_for:
            self._add(
                actions, "file-text", "Dùng cho OCR", lambda: view.use_for_ocr(self, self._model)
            )
        self._add(actions, "refresh-cw", "Cập nhật", lambda: view.update_model(self, self._model))
        remove = ConfirmToolButton(tooltip="Xóa mô hình", confirm_tooltip="Bấm lại để xóa mô hình")
        remove.confirmed.connect(lambda: view.remove(self, self._model))
        actions.addWidget(remove)
        self._buttons.append(remove)
        layout.addLayout(actions, 0)
        layout.setAlignment(actions, Qt.AlignmentFlag.AlignTop)

    def _add(
        self,
        layout: QHBoxLayout,
        icon_name: str,
        tooltip: str,
        handler,
        tone: str = "muted",
    ) -> None:
        """An action as its verb-icon. The tooltip and accessible name carry the wording."""
        button = QPushButton()
        button.setProperty("class", "icon")
        button.setIcon(icon(icon_name, color=token(tone), size=SPACE["lg"] - 2))
        button.setToolTip(tooltip)
        button.setAccessibleName(tooltip)
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
        # The settings tab host supplies the page padding; a second inset would double it.
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(PAGE_SPACING)

        heading = QHBoxLayout()
        heading.setSpacing(TOOLBAR_SPACING)
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["3xs"])
        eyebrow = QLabel("Mô hình cục bộ")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Quản lý mô hình")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel("Nạp, cập nhật và chọn mặc định.")
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

        ctx.modelsChanged.connect(self.refresh)
        # Icon pixmaps are tinted at build time, so a theme swap has to rebuild the rows.
        ctx.themeChanged.connect(self._on_theme_changed)
        self.refresh()

    def on_activated(self) -> None:
        self.refresh()

    def _on_theme_changed(self, _name: str = "") -> None:
        """Bound method, not a lambda: this way Qt drops the connection with the view."""
        self._render()

    def chat_models(self) -> list[ModelInfo]:
        return self._model.chat_models()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        if self._loading:
            return
        self._loading = True
        self._empty.setText("Đang đọc thư viện mô hình…")
        self._empty.setToolTip("")
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
        # The sentence says what happened and what to do next; the Python message is detail
        # for whoever goes looking, so it goes in the tooltip rather than between the two.
        self._empty.setText("Không đọc được thư viện mô hình.\nKhởi động nhà cung cấp rồi thử lại.")
        self._empty.setToolTip(str(exc))
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
            self._empty.setToolTip("")
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
