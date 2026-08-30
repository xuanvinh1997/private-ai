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
from private_ai.ui.widgets.status_pip import StatusPip

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.preferences import AppPreferences
    from private_ai.ui.context import AppContext

logger = logging.getLogger(__name__)

STRATEGY_LABELS: dict[str, tuple[str, str]] = {
    "auto": ("Tự chọn", "Để trợ lý tự quyết định cách tìm dựa trên câu hỏi."),
    "vector": ("Vector", "Tìm theo ngữ nghĩa trên các đoạn đã nhúng."),
    "keyword": ("Từ khóa", "Khớp đúng chữ — hợp với mã số, tên riêng, thuật ngữ."),
    "hybrid": ("Kết hợp", "Gộp kết quả vector và từ khóa rồi xếp hạng lại."),
    "graph": ("Đồ thị tri thức", "Đi theo thực thể và quan hệ, hợp với câu hỏi nhiều bước."),
    "summary": ("Tóm tắt", "Đọc toàn bộ tài liệu thay vì từng đoạn rời rạc."),
    "web": ("Tìm kiếm web", "Gửi câu hỏi ra ngoài máy tới nguồn tìm kiếm đã cấu hình."),
}

BACKEND_LABELS: dict[str, tuple[str, str]] = {
    "searxng": ("SearXNG", "Máy chủ SearXNG của bạn — riêng tư nhất nếu chạy trên máy này."),
    "duckduckgo": ("DuckDuckGo", "Không cần khóa, không cần máy chủ; câu hỏi rời khỏi máy."),
    "openai": ("OpenAI", "Dùng công cụ tìm kiếm của OpenAI; tốn phí theo lượt tìm."),
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


def _section(title: str, blurb: str) -> tuple[QFrame, QVBoxLayout]:
    frame = QFrame()
    frame.setProperty("class", "card")
    layout = QVBoxLayout(frame)
    layout.setSpacing(6)
    heading = QLabel(title)
    heading.setProperty("class", "subtitle")
    layout.addWidget(heading)
    if blurb:
        description = QLabel(blurb)
        description.setWordWrap(True)
        description.setProperty("class", "muted")
        layout.addWidget(description)
    return frame, layout


class _Segmented(QWidget):
    """A row of checkable buttons behaving as one exclusive choice."""

    def __init__(self, options: list[tuple[str, str]], parent=None) -> None:
        super().__init__(parent)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(4)
        self._buttons: dict[str, QPushButton] = {}
        self._handler = None
        for value, label in options:
            button = QPushButton(label)
            button.setCheckable(True)
            button.setProperty("class", "chip")
            button.clicked.connect(lambda _=False, v=value: self._picked(v))
            layout.addWidget(button)
            self._buttons[value] = button
        layout.addStretch(1)

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
        root.setSpacing(12)
        scroll.setWidget(canvas)
        outer.addWidget(scroll)

        # --- appearance --------------------------------------------------
        frame, layout = _section("Giao diện", "Chọn nền sáng dễ đọc hoặc nền tối.")
        self._theme = _Segmented([("light", "Sáng"), ("dark", "Tối"), ("system", "Theo hệ thống")])
        self._theme.on_change(self._ctx.set_theme)
        layout.addWidget(self._theme)
        root.addWidget(frame)

        frame, layout = _section("Cỡ chữ", "Tăng toàn bộ chữ và vùng điều khiển.")
        self._large_text = QCheckBox("Dùng chữ lớn")
        self._large_text.toggled.connect(
            lambda value: self._ctx.set_font_scale("large" if value else "normal")
        )
        layout.addWidget(self._large_text)
        root.addWidget(frame)

        # --- OCR ----------------------------------------------------------
        frame, layout = _section(
            "Đọc văn bản bằng OCR",
            "Bật OCR khi tệp không có lớp văn bản. Tắt thì chỉ đọc văn bản có sẵn — nhanh "
            "hơn nhưng bỏ qua tài liệu scan.",
        )
        self._ocr = QCheckBox("Đang bật")
        self._ocr.toggled.connect(self._on_ocr)
        layout.addWidget(self._ocr)
        root.addWidget(frame)

        # --- retrieval ----------------------------------------------------
        frame, layout = _section(
            "Cách tìm mặc định",
            "Chiến lược truy xuất dùng cho mọi câu hỏi khi bạn không chọn khác trong ô soạn.",
        )
        self._strategy = QComboBox()
        for name in RetrievalStrategyName:
            label, hint = STRATEGY_LABELS.get(name.value, (name.value, ""))
            self._strategy.addItem(f"{label} — {hint}" if hint else label, name.value)
        self._strategy.currentIndexChanged.connect(self._on_strategy)
        layout.addWidget(self._strategy)
        root.addWidget(frame)

        frame, layout = _section(
            "Mô hình trích xuất đồ thị",
            "Chọn model nhỏ cho bước rút thực thể và quan hệ. Model trả lời chat không đổi.",
        )
        self._graph_model = QComboBox()
        self._graph_model.addItem("Mô hình chat mặc định", "")
        self._graph_model.currentIndexChanged.connect(self._on_graph_model)
        layout.addWidget(self._graph_model)
        root.addWidget(frame)

        # --- web search ---------------------------------------------------
        frame, layout = _section(
            "Tìm kiếm web",
            "Đây là tính năng duy nhất khiến câu hỏi rời khỏi máy này: nội dung tin nhắn "
            "được gửi tới nguồn tìm kiếm bên dưới. Tài liệu, bộ nhớ và tri thức vẫn ở lại máy.",
        )
        self._web_enabled = QCheckBox("Đang tắt")
        self._web_enabled.toggled.connect(self._on_web_enabled)
        layout.addWidget(self._web_enabled)

        self._backend = _Segmented(
            [(value, BACKEND_LABELS[value][0]) for value in ("searxng", "duckduckgo", "openai")]
        )
        self._backend.on_change(self._on_backend)
        layout.addWidget(self._backend)
        self._backend_hint = QLabel("")
        self._backend_hint.setWordWrap(True)
        self._backend_hint.setProperty("class", "faint")
        layout.addWidget(self._backend_hint)

        self._searxng_label = QLabel("Địa chỉ SearXNG")
        self._searxng = QLineEdit()
        self._searxng.setPlaceholderText("http://127.0.0.1:8888")
        self._searxng.editingFinished.connect(self._on_searxng_url)
        layout.addWidget(self._searxng_label)
        layout.addWidget(self._searxng)
        self._searxng_hint = QLabel(
            "SearXNG chỉ trả HTML cho tới khi bạn thêm json vào search.formats trong settings.yml."
        )
        self._searxng_hint.setWordWrap(True)
        self._searxng_hint.setProperty("class", "faint")
        layout.addWidget(self._searxng_hint)

        self._key_label = QLabel("OpenAI API key")
        key_row = QHBoxLayout()
        self._key = QLineEdit()
        self._key.setEchoMode(QLineEdit.EchoMode.Password)
        key_row.addWidget(self._key, 1)
        self._save_key = QPushButton("Lưu key")
        self._save_key.clicked.connect(self._on_save_key)
        key_row.addWidget(self._save_key)
        self._clear_key = QPushButton("Xóa key")
        self._clear_key.clicked.connect(self._on_clear_key)
        key_row.addWidget(self._clear_key)
        self._key_row = QWidget()
        self._key_row.setLayout(key_row)
        layout.addWidget(self._key_label)
        layout.addWidget(self._key_row)

        self._search_model_label = QLabel("Mô hình chạy tìm kiếm")
        self._search_model = QLineEdit()
        self._search_model.setPlaceholderText("gpt-5")
        self._search_model.editingFinished.connect(self._on_search_model)
        layout.addWidget(self._search_model_label)
        layout.addWidget(self._search_model)

        probe_row = QHBoxLayout()
        self._probe = QPushButton("Kiểm tra kết nối")
        self._probe.clicked.connect(self._on_probe)
        probe_row.addWidget(self._probe)
        self._probe_result = QLabel("")
        self._probe_result.setWordWrap(True)
        self._probe_result.setProperty("class", "faint")
        probe_row.addWidget(self._probe_result, 1)
        layout.addLayout(probe_row)
        root.addWidget(frame)

        # --- embedding performance ----------------------------------------
        frame, layout = _section(
            "Hiệu năng embedding",
            "Tăng dần nếu máy còn RAM/VRAM. Giá trị quá cao có thể làm mô hình chậm hoặc "
            "hết bộ nhớ.",
        )
        numbers = QHBoxLayout()
        numbers.addWidget(QLabel("Kích thước lô"))
        self._batch = QSpinBox()
        self._batch.setRange(1, 256)
        self._batch.setToolTip("1–256 đoạn mỗi lô")
        self._batch.editingFinished.connect(self._on_batch)
        numbers.addWidget(self._batch)
        numbers.addSpacing(16)
        numbers.addWidget(QLabel("Tác vụ song song"))
        self._concurrency = QSpinBox()
        self._concurrency.setRange(1, 32)
        self._concurrency.setToolTip("1–32 yêu cầu đồng thời")
        self._concurrency.editingFinished.connect(self._on_concurrency)
        numbers.addWidget(self._concurrency)
        numbers.addStretch(1)
        layout.addLayout(numbers)
        root.addWidget(frame)

        # --- provider readout ---------------------------------------------
        frame, layout = _section(
            "Nhà cung cấp đang dùng", "Nơi mô hình thực sự chạy cho phiên làm việc này."
        )
        provider_row = QHBoxLayout()
        self._provider_pip = StatusPip("unknown")
        provider_row.addWidget(self._provider_pip)
        self._provider_name = QLabel("Đang kiểm tra…")
        provider_row.addWidget(self._provider_name, 1)
        layout.addLayout(provider_row)
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
            self._ocr.setText("Đang bật" if preferences.ocr_enabled else "Đang tắt")
            index = self._strategy.findData(str(preferences.retrieval_strategy))
            if index >= 0:
                self._strategy.setCurrentIndex(index)
            self._select_graph_model(preferences.graph_model)
            self._web_enabled.setChecked(preferences.web_search_enabled)
            self._web_enabled.setText("Đang bật" if preferences.web_search_enabled else "Đang tắt")
            backend = str(preferences.web_search_backend)
            self._backend.set_value(backend)
            self._backend_hint.setText(BACKEND_LABELS.get(backend, ("", ""))[1])
            self._searxng.setText(preferences.web_search_base_url)
            self._search_model.setText(preferences.web_search_model)
            self._key.setPlaceholderText(
                "Đã lưu một API key" if preferences.web_search_has_api_key else "sk-…"
            )
            self._clear_key.setVisible(preferences.web_search_has_api_key)
            self._batch.setValue(preferences.embedding_batch_size)
            self._concurrency.setValue(preferences.embedding_concurrency)
            for widget in (self._searxng_label, self._searxng, self._searxng_hint):
                widget.setVisible(backend == "searxng")
            for widget in (
                self._key_label,
                self._key_row,
                self._search_model_label,
                self._search_model,
            ):
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
        self._ocr.setText("Đang bật" if value else "Đang tắt")
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
        self._web_enabled.setText("Đang bật" if value else "Đang tắt")
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
            self._probe_result.setProperty("class", "faint")
        else:
            self._probe_result.setText(str(result.get("detail") or "Không kết nối được"))
            self._probe_result.setProperty("class", "danger")

    def _probe_failed(self, exc: BaseException) -> None:
        self._probe.setEnabled(True)
        self._probe_result.setText(str(exc) or "Không kiểm tra được kết nối")


class SettingsView(QWidget):
    """The tab host. Sub-views are built the first time their tab is shown."""

    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._built: dict[str, QWidget] = {}

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 20, 24, 20)
        root.setSpacing(12)

        eyebrow = QLabel("Cài đặt thiết bị")
        eyebrow.setProperty("class", "section-label")
        root.addWidget(eyebrow)
        title = QLabel("Cài đặt")
        title.setProperty("class", "title")
        root.addWidget(title)
        blurb = QLabel(
            "Hiển thị, xử lý tài liệu và các cấu hình nâng cao đều nằm ở đây. "
            "Mọi lựa chọn chỉ lưu trên máy hiện tại."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        root.addWidget(blurb)

        self._tabs = QTabWidget()
        self._tabs.setDocumentMode(True)
        self._hosts: dict[str, QWidget] = {}
        for key in TAB_ORDER:
            host = QWidget()
            layout = QVBoxLayout(host)
            layout.setContentsMargins(0, 8, 0, 0)
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
