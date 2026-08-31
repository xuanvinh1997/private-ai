"""Settings: one general panel plus the five sub-views that have their own screens.

Every write is optimistic — the control moves first, then the row is written — because a
preference that lags a click by a database round trip feels broken. If the write fails
the control is put back and the toast says why.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QScrollArea,
    QSpinBox,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from private_ai.core.preferences import write_app_preferences
from private_ai.core.schemas import (
    PreferencesUpdate,
    RetrievalStrategyName,
    WebSearchBackend,
)
from private_ai.rag.web_search import WebSearchConfig
from private_ai.ui.theme import (
    CARD_MARGINS,
    PAGE_MARGINS,
    PAGE_SPACING,
    SPACE,
    TOOLBAR_SPACING,
    restyle,
)
from private_ai.ui.widgets.status_pip import StatusPip
from private_ai.ui.widgets.strategy_picker import STRATEGY_CHOICES

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.preferences import AppPreferences
    from private_ai.ui.context import AppContext

# Spin boxes hold two or three digits; a full-width field would read as free text.
_SPIN_WIDTH = SPACE["4xl"] * 2

logger = logging.getLogger(__name__)

BACKEND_LABELS: dict[str, tuple[str, str]] = {
    "searxng": ("SearXNG", "Máy chủ của bạn — riêng tư nhất nếu chạy tại máy."),
    # The card's own subtitle already says the question leaves the machine; a hint that
    # repeats it under every choice is the line nobody reads twice.
    "duckduckgo": ("DuckDuckGo", "Không cần khóa."),
    "openai": ("OpenAI", "Cần API key. Tốn phí mỗi lượt."),
}

TAB_ORDER = ("general", "models", "memory", "providers", "skills", "mcp")
TAB_LABELS = {
    "general": "Chung",
    "models": "Mô hình",
    "memory": "Bộ nhớ",
    "providers": "Nhà cung cấp",
    "skills": "Kỹ năng",
    "mcp": "MCP",
}


def _group(
    title: str,
    hint: str = "",
    control: QWidget | None = None,
) -> tuple[QFrame, QVBoxLayout]:
    """One card per *group* of settings, not per setting.

    Eight outlined cards down a page is eight frames competing with the one accent colour
    the screen actually needs. Related preferences share a card and are told apart by a
    hairline, which leaves the borders meaning "these belong together".

    ``control`` is the group's own master switch, and it belongs on the heading line: a
    whole row captioned "Bật" under a heading that already names the feature says the same
    thing twice and spends a row doing it.
    """
    frame = QFrame()
    frame.setProperty("class", "card")
    layout = QVBoxLayout(frame)
    layout.setContentsMargins(*CARD_MARGINS)
    layout.setSpacing(SPACE["sm"])

    head = QHBoxLayout()
    head.setContentsMargins(0, 0, 0, 0)
    head.setSpacing(SPACE["md"])
    copy = QVBoxLayout()
    copy.setContentsMargins(0, 0, 0, 0)
    copy.setSpacing(SPACE["3xs"])
    heading = QLabel(title)
    heading.setProperty("class", "card-title")
    copy.addWidget(heading)
    if hint:
        note = QLabel(hint)
        note.setWordWrap(True)
        note.setProperty("class", "muted")
        copy.addWidget(note)
    head.addLayout(copy, 1)
    if control is not None:
        head.addWidget(control, 0, Qt.AlignmentFlag.AlignVCenter)
    layout.addLayout(head)
    return frame, layout


def _divider() -> QFrame:
    """The hairline between two rows of one card."""
    line = QFrame()
    line.setProperty("class", "hline")
    return line


def _row(
    caption: str,
    control: QWidget,
    hint: str = "",
    hint_widget: QWidget | None = None,
) -> QWidget:
    """A setting as one row: what it is on the left, what it is set to on the right.

    Only controls that need the width — combo boxes, URLs, keys — drop to their own line;
    a checkbox or a segmented choice sits beside its caption, which is what turns a page of
    stacked headings into a list you can scan.
    """
    holder = QWidget()
    box = QVBoxLayout(holder)
    box.setContentsMargins(0, 0, 0, 0)
    box.setSpacing(SPACE["2xs"])

    wide = isinstance(control, QComboBox | QLineEdit) or control.property("wideRow") is True
    line = QHBoxLayout()
    line.setContentsMargins(0, 0, 0, 0)
    line.setSpacing(SPACE["md"])
    copy = QVBoxLayout()
    copy.setContentsMargins(0, 0, 0, 0)
    copy.setSpacing(SPACE["3xs"])
    label = QLabel(caption)
    label.setProperty("class", "body-strong")
    copy.addWidget(label)
    if hint:
        note = QLabel(hint)
        note.setWordWrap(True)
        note.setProperty("class", "muted")
        copy.addWidget(note)
    if hint_widget is not None:
        copy.addWidget(hint_widget)
    line.addLayout(copy, 1)
    if not wide:
        line.addWidget(control, 0, Qt.AlignmentFlag.AlignVCenter)
    box.addLayout(line)
    if wide:
        box.addWidget(control)
    return holder


class _Segmented(QWidget):
    """A row of checkable buttons behaving as one exclusive choice."""

    def __init__(self, options: list[tuple[str, str]], parent=None) -> None:
        super().__init__(parent)
        self.setProperty("class", "segment")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        layout = QHBoxLayout(self)
        # The track's own hairline is the padding; the buttons sit inside it.
        layout.setContentsMargins(*(SPACE["3xs"],) * 4)
        layout.setSpacing(SPACE["3xs"])
        self._buttons: dict[str, QPushButton] = {}
        self._handler = None
        for value, label in options:
            button = QPushButton(label)
            button.setCheckable(True)
            button.setProperty("class", "segment-item")
            button.setCursor(Qt.CursorShape.PointingHandCursor)
            button.clicked.connect(lambda _=False, v=value: self._picked(v))
            layout.addWidget(button)
            self._buttons[value] = button

    def on_change(self, handler) -> None:
        self._handler = handler

    def _picked(self, value: str) -> None:
        self.set_value(value)
        if self._handler is not None:
            self._handler(value)

    def set_value(self, value: str) -> None:
        for key, button in self._buttons.items():
            button.setChecked(key == value)

    def value(self) -> str:
        return next((key for key, button in self._buttons.items() if button.isChecked()), "")

    def set_enabled(self, enabled: bool) -> None:
        for button in self._buttons.values():
            button.setEnabled(enabled)


class GeneralSettings(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._syncing = False

        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        canvas = QWidget()
        root = QVBoxLayout(canvas)
        # Flush with the tab, minus a gutter so the cards do not sit under the scrollbar.
        root.setContentsMargins(0, 0, SPACE["md"], 0)
        root.setSpacing(PAGE_SPACING)
        scroll.setWidget(canvas)
        outer.addWidget(scroll)

        # --- appearance ---------------------------------------------------
        frame, layout = _group("Giao diện")
        self._theme = _Segmented([("light", "Sáng"), ("dark", "Tối"), ("system", "Hệ thống")])
        self._theme.on_change(self._ctx.set_theme)
        layout.addWidget(_row("Nền", self._theme))
        layout.addWidget(_divider())
        self._large_text = QCheckBox()
        self._large_text.toggled.connect(
            lambda value: self._ctx.set_font_scale("large" if value else "normal")
        )
        layout.addWidget(_row("Chữ lớn", self._large_text, "Phóng to cả chữ và vùng bấm."))
        root.addWidget(frame)

        # --- documents ------------------------------------------------------
        frame, layout = _group("Tài liệu")
        self._ocr = QCheckBox()
        self._ocr.toggled.connect(self._on_ocr)
        layout.addWidget(_row("OCR", self._ocr, "Đọc được tài liệu scan. Chậm hơn."))
        layout.addWidget(_divider())

        numbers = QWidget()
        # Two numbers side by side is the whole control; the row helper would stack it.
        numbers.setProperty("wideRow", False)
        number_row = QHBoxLayout(numbers)
        number_row.setContentsMargins(0, 0, 0, 0)
        number_row.setSpacing(TOOLBAR_SPACING)
        self._batch = QSpinBox()
        self._batch.setRange(1, 256)
        self._batch.setToolTip("Số đoạn mỗi lô, 1–256")
        self._batch.editingFinished.connect(self._on_batch)
        self._concurrency = QSpinBox()
        self._concurrency.setRange(1, 32)
        self._concurrency.setToolTip("Số yêu cầu chạy cùng lúc, 1–32")
        self._concurrency.editingFinished.connect(self._on_concurrency)
        for caption, control in (("Lô", self._batch), ("Song song", self._concurrency)):
            label = QLabel(caption)
            label.setProperty("class", "muted")
            # Sized to the widest value it accepts plus the stepper column, so a two-digit
            # setting is not marooned at one end of a field built for nothing.
            # (``setAlignment`` is not used: a stylesheet-styled QSpinBox draws its text
            # left regardless of what the line edit reports.)
            control.setFixedWidth(_SPIN_WIDTH)
            number_row.addWidget(label, 0, Qt.AlignmentFlag.AlignVCenter)
            number_row.addWidget(control, 0, Qt.AlignmentFlag.AlignVCenter)
        layout.addWidget(_row("Embedding", numbers, "Tăng nếu máy còn RAM."))
        root.addWidget(frame)

        # --- retrieval ------------------------------------------------------
        frame, layout = _group("Truy xuất")
        # Same list and same wording as the composer's picker: two names for one strategy
        # is how "Tự chọn" here and "Tự động" there came to look like two features.
        # ``web`` is not among them — the composer's web toggle owns that.
        self._strategy = QComboBox()
        for value, label, hint in STRATEGY_CHOICES:
            self._strategy.addItem(label, value)
            self._strategy.setItemData(
                self._strategy.count() - 1, hint, Qt.ItemDataRole.ToolTipRole
            )
        self._strategy.currentIndexChanged.connect(self._on_strategy)
        layout.addWidget(_row("Cách tìm mặc định", self._strategy, "Khi ô soạn không chọn khác."))
        layout.addWidget(_divider())

        self._graph_model = QComboBox()
        self._graph_model.addItem("Mô hình chat mặc định", "")
        self._graph_model.currentIndexChanged.connect(self._on_graph_model)
        layout.addWidget(
            _row("Mô hình trích xuất", self._graph_model, "Chỉ dùng cho bước rút thực thể.")
        )
        root.addWidget(frame)

        # --- web search -----------------------------------------------------
        self._web_enabled = QCheckBox()
        self._web_enabled.setToolTip("Bật tìm kiếm web")
        self._web_enabled.setAccessibleName("Bật tìm kiếm web")
        self._web_enabled.toggled.connect(self._on_web_enabled)
        frame, layout = _group(
            "Tìm kiếm web",
            "Câu hỏi rời khỏi máy. Tài liệu thì không.",
            self._web_enabled,
        )

        self._backend = _Segmented(
            [(value, BACKEND_LABELS[value][0]) for value in ("searxng", "duckduckgo", "openai")]
        )
        self._backend.on_change(self._on_backend)
        self._backend_hint = QLabel("")
        self._backend_hint.setWordWrap(True)
        self._backend_hint.setProperty("class", "muted")
        layout.addWidget(_row("Nguồn", self._backend, hint_widget=self._backend_hint))

        self._searxng = QLineEdit()
        self._searxng.setPlaceholderText("http://127.0.0.1:8888")
        self._searxng.editingFinished.connect(self._on_searxng_url)
        self._searxng_row = _row(
            "Địa chỉ SearXNG", self._searxng, "settings.yml cần có json trong search.formats."
        )
        layout.addWidget(self._searxng_row)

        key_line = QWidget()
        key_line.setProperty("wideRow", True)
        key_row = QHBoxLayout(key_line)
        key_row.setContentsMargins(0, 0, 0, 0)
        key_row.setSpacing(TOOLBAR_SPACING)
        self._key = QLineEdit()
        self._key.setEchoMode(QLineEdit.EchoMode.Password)
        key_row.addWidget(self._key, 1)
        self._save_key = QPushButton("Lưu")
        self._save_key.clicked.connect(self._on_save_key)
        key_row.addWidget(self._save_key)
        self._clear_key = QPushButton("Xóa")
        self._clear_key.clicked.connect(self._on_clear_key)
        key_row.addWidget(self._clear_key)
        self._key_row = _row("OpenAI API key", key_line)
        layout.addWidget(self._key_row)

        self._search_model = QLineEdit()
        self._search_model.setPlaceholderText("gpt-5")
        self._search_model.editingFinished.connect(self._on_search_model)
        self._search_model_row = _row("Model tìm kiếm", self._search_model)
        layout.addWidget(self._search_model_row)

        layout.addWidget(_divider())
        self._probe = QPushButton("Kiểm tra")
        self._probe.clicked.connect(self._on_probe)
        self._probe_result = QLabel("")
        self._probe_result.setWordWrap(True)
        self._probe_result.setProperty("class", "muted")
        # The verdict rides in the caption column so a two-line failure wraps against the
        # text, not against the button it would otherwise push around.
        layout.addWidget(_row("Kết nối", self._probe, hint_widget=self._probe_result))
        root.addWidget(frame)

        # --- provider readout -----------------------------------------------
        frame, layout = _group("Nhà cung cấp")
        # A readout, not a setting: the heading already says what this is, so the row that
        # would have repeated it as "Đang chạy tại" is gone and only the answer is left.
        provider_box = QHBoxLayout()
        provider_box.setContentsMargins(0, 0, 0, 0)
        provider_box.setSpacing(SPACE["sm"])
        self._provider_pip = StatusPip("unknown")
        provider_box.addWidget(self._provider_pip, 0, Qt.AlignmentFlag.AlignVCenter)
        self._provider_name = QLabel("Đang kiểm tra…")
        provider_box.addWidget(self._provider_name, 1, Qt.AlignmentFlag.AlignVCenter)
        layout.addLayout(provider_box)
        root.addWidget(frame)

        root.addStretch(1)

        ctx.preferencesChanged.connect(self._sync)
        ctx.modelsChanged.connect(self._load_models)
        self._sync(ctx.preferences)
        self._load_models()
        self._refresh_provider()

    # --- syncing ----------------------------------------------------------

    def on_activated(self) -> None:
        self._sync(self._ctx.preferences)
        self._refresh_provider()

    def _sync(self, preferences: AppPreferences) -> None:
        self._syncing = True
        try:
            self._theme.set_value(self._ctx.preferences.ui_theme)
            self._large_text.setChecked(preferences.ui_font_scale == "large")
            self._ocr.setChecked(preferences.ocr_enabled)
            # A default saved as ``web`` under the old meaning is no longer offered here;
            # the composer migrates it onto the web toggle, so show what will be used.
            index = self._strategy.findData(str(preferences.retrieval_strategy))
            self._strategy.setCurrentIndex(index if index >= 0 else 0)
            self._select_graph_model(preferences.graph_model)
            self._web_enabled.setChecked(preferences.web_search_enabled)
            backend = str(preferences.web_search_backend)
            self._backend.set_value(backend)
            self._backend_hint.setText(BACKEND_LABELS.get(backend, ("", ""))[1])
            self._searxng.setText(preferences.web_search_base_url)
            self._search_model.setText(preferences.web_search_model)
            self._key.setPlaceholderText(
                "Đã lưu một key" if preferences.web_search_has_api_key else "sk-…"
            )
            self._clear_key.setVisible(preferences.web_search_has_api_key)
            self._batch.setValue(preferences.embedding_batch_size)
            self._concurrency.setValue(preferences.embedding_concurrency)
            self._searxng_row.setVisible(backend == "searxng")
            for widget in (self._key_row, self._search_model_row):
                widget.setVisible(backend == "openai")
        finally:
            self._syncing = False

    def _select_graph_model(self, name: str) -> None:
        index = self._graph_model.findData(name)
        if index < 0 and name:
            self._graph_model.addItem(f"{name} · hiện không khả dụng", name)
            index = self._graph_model.count() - 1
        self._graph_model.setCurrentIndex(max(0, index))

    def _load_models(self) -> None:
        self._ctx.run(
            self._ctx.services.models.list_models(),
            on_result=self._models_loaded,
            # A provider that is down should not put a red toast on the settings screen.
            on_error=lambda exc: logger.debug("Không đọc được danh sách mô hình: %s", exc),
        )

    def _models_loaded(self, models) -> None:
        current = str(self._ctx.preferences.graph_model)
        self._syncing = True
        try:
            self._graph_model.clear()
            self._graph_model.addItem("Mô hình chat mặc định", "")
            for model in models:
                if model.model_type == "embedding" or model.capabilities == ["embedding"]:
                    continue
                self._graph_model.addItem(model.name, model.name)
            self._select_graph_model(current)
        finally:
            self._syncing = False

    def _refresh_provider(self) -> None:
        config = self._ctx.services.providers.active_config()
        self._provider_name.setText(config.name if config else "Chưa cấu hình")
        if config is None:
            self._provider_pip.set_state("not_configured")
            return
        self._ctx.run(
            self._ctx.services.models.health(),
            on_result=lambda ok: self._provider_pip.set_state("online" if ok else "offline"),
            on_error=lambda _: self._provider_pip.set_state("error"),
        )

    # --- writes -----------------------------------------------------------

    def _write(self, update: PreferencesUpdate, revert, message: str) -> None:
        self._ctx.run(
            write_app_preferences(self._ctx.database, update),
            on_result=lambda _: self._written(),
            on_error=lambda exc: self._rollback(revert, exc, message),
        )

    def _written(self) -> None:
        self._ctx.refresh_preferences()

    def _rollback(self, revert, exc: BaseException, message: str) -> None:
        self._syncing = True
        try:
            revert()
        finally:
            self._syncing = False
        self._ctx.toast(f"{message}: {exc}" if str(exc) else message, "error")

    # --- handlers ---------------------------------------------------------

    def _on_ocr(self, value: bool) -> None:
        if self._syncing:
            return
        self._write(
            PreferencesUpdate(ocr_enabled=value),
            lambda: self._ocr.setChecked(not value),
            "Không lưu được lựa chọn OCR",
        )

    def _on_strategy(self) -> None:
        if self._syncing:
            return
        value = str(self._strategy.currentData())
        previous = str(self._ctx.preferences.retrieval_strategy)
        self._write(
            PreferencesUpdate(retrieval_strategy=RetrievalStrategyName(value)),
            lambda: self._select_data(self._strategy, previous),
            "Không lưu được cách tìm mặc định",
        )

    def _on_graph_model(self) -> None:
        if self._syncing:
            return
        value = str(self._graph_model.currentData() or "")
        previous = self._ctx.preferences.graph_model
        self._write(
            PreferencesUpdate(graph_model=value),
            lambda: self._select_data(self._graph_model, previous),
            "Không lưu được mô hình trích xuất",
        )

    @staticmethod
    def _select_data(combo: QComboBox, value: str) -> None:
        index = combo.findData(value)
        if index >= 0:
            combo.setCurrentIndex(index)

    def _on_web_enabled(self, value: bool) -> None:
        if self._syncing:
            return
        self._write(
            PreferencesUpdate(web_search_enabled=value),
            lambda: self._web_enabled.setChecked(not value),
            "Không lưu được lựa chọn tìm kiếm web",
        )

    def _on_backend(self, value: str) -> None:
        if self._syncing:
            return
        previous = str(self._ctx.preferences.web_search_backend)
        self._backend_hint.setText(BACKEND_LABELS.get(value, ("", ""))[1])
        self._write(
            PreferencesUpdate(web_search_backend=WebSearchBackend(value)),
            lambda: self._backend.set_value(previous),
            "Không lưu được nguồn tìm kiếm",
        )

    def _on_searxng_url(self) -> None:
        if self._syncing:
            return
        value = self._searxng.text().strip()
        previous = self._ctx.preferences.web_search_base_url
        if value == previous:
            return
        self._write(
            PreferencesUpdate(web_search_base_url=value),
            lambda: self._searxng.setText(previous),
            "Không lưu được địa chỉ SearXNG",
        )

    def _on_search_model(self) -> None:
        if self._syncing:
            return
        value = self._search_model.text().strip()
        previous = self._ctx.preferences.web_search_model
        if value == previous:
            return
        self._write(
            PreferencesUpdate(web_search_model=value),
            lambda: self._search_model.setText(previous),
            "Không lưu được mô hình tìm kiếm",
        )

    def _on_save_key(self) -> None:
        value = self._key.text().strip()
        if not value:
            return
        self._key.clear()
        self._write(
            PreferencesUpdate(web_search_api_key=value),
            lambda: self._key.setText(value),
            "Không lưu được API key",
        )

    def _on_clear_key(self) -> None:
        self._write(
            PreferencesUpdate(web_search_api_key=""),
            lambda: None,
            "Không xóa được API key",
        )

    def _on_batch(self) -> None:
        if self._syncing:
            return
        value = self._batch.value()
        previous = self._ctx.preferences.embedding_batch_size
        if value == previous:
            return
        self._ctx.services.graph.configure_indexing(value, self._concurrency.value())
        self._write(
            PreferencesUpdate(embedding_batch_size=value),
            lambda: self._batch.setValue(previous),
            "Không lưu được kích thước lô",
        )

    def _on_concurrency(self) -> None:
        if self._syncing:
            return
        value = self._concurrency.value()
        previous = self._ctx.preferences.embedding_concurrency
        if value == previous:
            return
        self._ctx.services.graph.configure_indexing(self._batch.value(), value)
        self._write(
            PreferencesUpdate(embedding_concurrency=value),
            lambda: self._concurrency.setValue(previous),
            "Không lưu được số tác vụ song song",
        )

    # --- probe ------------------------------------------------------------

    def _on_probe(self) -> None:
        preferences = self._ctx.preferences
        backend = self._backend.value() or str(preferences.web_search_backend)
        config = WebSearchConfig(
            backend=backend,
            base_url=self._searxng.text().strip() or preferences.web_search_base_url,
            api_key=self._key.text().strip() or preferences.web_search_api_key,
            model=self._search_model.text().strip() or preferences.web_search_model,
            max_results=preferences.web_search_max_results,
        )
        self._probe.setEnabled(False)
        self._probe_result.setText("Đang kiểm tra…")
        self._probe_result.setProperty("class", "muted")
        restyle(self._probe_result)
        self._ctx.run(
            self._ctx.services.web_search.probe(config),
            on_result=self._probe_done,
            on_error=self._probe_failed,
        )

    def _probe_done(self, result: dict[str, Any]) -> None:
        self._probe.setEnabled(True)
        if result.get("reachable"):
            locality = "chạy trên máy này" if result.get("on_device") else "dữ liệu rời khỏi máy"
            self._probe_result.setText(
                f"{result.get('host', '')} trả về {result.get('result_count', 0)} kết quả · "
                f"{locality}"
            )
            self._probe_result.setProperty("class", "muted")
        else:
            self._probe_result.setText(str(result.get("detail") or "Không kết nối được"))
            self._probe_result.setProperty("class", "danger")
        # Qt caches the computed style, so the class swap is inert without a re-polish.
        restyle(self._probe_result)

    def _probe_failed(self, exc: BaseException) -> None:
        self._probe.setEnabled(True)
        self._probe_result.setText(str(exc) or "Không kiểm tra được kết nối")
        self._probe_result.setProperty("class", "danger")
        restyle(self._probe_result)


class SettingsView(QWidget):
    """The tab host. Sub-views are built the first time their tab is shown."""

    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._built: dict[str, QWidget] = {}

        root = QVBoxLayout(self)
        root.setContentsMargins(*PAGE_MARGINS)
        root.setSpacing(PAGE_SPACING)

        # The eyebrow said "Cài đặt thiết bị" above a title reading "Cài đặt", and the blurb
        # listed the tab labels sitting two rows below it. What is left is the one fact the
        # screen cannot show by itself.
        titles = QVBoxLayout()
        titles.setSpacing(SPACE["3xs"])
        title = QLabel("Cài đặt")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel("Mọi lựa chọn chỉ lưu trên máy này.")
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        root.addLayout(titles)

        self._tabs = QTabWidget()
        self._tabs.setDocumentMode(True)
        # The native base is drawn from the *system* appearance, not the app's theme; the
        # pane rule in the stylesheet supplies the only edge this strip needs.
        self._tabs.tabBar().setDrawBase(False)
        self._hosts: dict[str, QWidget] = {}
        for key in TAB_ORDER:
            host = QWidget()
            layout = QVBoxLayout(host)
            layout.setContentsMargins(0, SPACE["sm"], 0, 0)
            self._hosts[key] = host
            self._tabs.addTab(host, TAB_LABELS[key])
        self._tabs.currentChanged.connect(self._on_tab)
        root.addWidget(self._tabs, 1)

        self._build("general")

    # --- tabs -------------------------------------------------------------

    def show_tab(self, tab: str) -> None:
        if tab not in TAB_ORDER:
            return
        self._tabs.setCurrentIndex(TAB_ORDER.index(tab))

    def on_activated(self) -> None:
        self._on_tab(self._tabs.currentIndex())

    def _on_tab(self, index: int) -> None:
        if not 0 <= index < len(TAB_ORDER):
            return
        key = TAB_ORDER[index]
        view = self._build(key)
        activated = getattr(view, "on_activated", None)
        if callable(activated):
            activated()

    def _build(self, key: str) -> QWidget | None:
        existing = self._built.get(key)
        if existing is not None:
            return existing
        try:
            view = self._construct(key)
        except Exception:  # noqa: BLE001 - one broken tab must not take the screen down
            logger.exception("Không dựng được tab cài đặt %s", key)
            view = QLabel(f"Không mở được mục “{TAB_LABELS[key]}”.")
            view.setAlignment(Qt.AlignmentFlag.AlignCenter)
            view.setProperty("class", "empty")
        self._built[key] = view
        self._hosts[key].layout().addWidget(view)
        return view

    def _construct(self, key: str) -> QWidget:
        if key == "general":
            return GeneralSettings(self._ctx, self)
        if key == "models":
            from private_ai.ui.views.models_view import ModelsView

            return ModelsView(self._ctx, self)
        if key == "memory":
            from private_ai.ui.views.memory_view import MemoryView

            return MemoryView(self._ctx, self)
        if key == "providers":
            from private_ai.ui.views.providers_view import ProvidersView

            return ProvidersView(self._ctx, self)
        if key == "skills":
            from private_ai.ui.views.skills_view import SkillsView

            return SkillsView(self._ctx, self)
        from private_ai.ui.views.mcp_view import McpView

        return McpView(self._ctx, self)
