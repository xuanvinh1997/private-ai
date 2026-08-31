"""The application shell: sidebar, topbar, a stack of views, and the toast overlay.

=============================================================================
UI SHELL API — the contract ``ui/views/*``, ``ui/dialogs/*`` and ``ui/models/*``
build against. The same text lives in ``scratchpad/UI_SHELL_API.md``.
=============================================================================

**Constructing a view.** Every screen is ``class XView(QWidget)`` with
``__init__(self, ctx: AppContext, parent: QWidget | None = None)``. Optional lifecycle
hooks ``on_activated()`` / ``on_deactivated()`` are called if they exist — put polling
starts and stops there, not in ``showEvent``.

**AppContext** (``private_ai.ui.context.AppContext``) carries:
``services``, ``preferences``, ``window``, ``database``, ``settings``;
``workspace_id`` / ``conversation_id`` / ``theme_name`` / ``font_scale``;
signals ``workspaceChanged(str) conversationChanged(str) themeChanged(str)
preferencesChanged(object) documentsChanged() modelsChanged()
navigateRequested(str, str)``; and the shell calls
``set_workspace set_conversation set_theme set_font_scale update_preferences
refresh_preferences run(coro, on_result=, on_error=) toast(message, tone)
navigate(view_key, sub_tab) notify(Notice)``.

**Helpers.** ``private_ai.ui.theme`` → ``tokens() token(key) type_scale() apply_theme()
restyle(widget)``. ``private_ai.ui.icons`` → ``icon(name, color=, size=)``,
``pixmap(...)``. ``private_ai.ui.format`` → ``format_bytes format_file_size
format_relative_time format_percent format_count stage_label status_label elide
short_model_name initials_of``. ``private_ai.ui.markdown`` → ``markdown_to_html(text)``,
``document_css(tokens)``. ``private_ai.ui.async_bridge`` → ``run_coro``, ``slot_async``.
Widgets: ``widgets.confirm_button.ConfirmButton``, ``widgets.status_pip.StatusPip``,
``widgets.model_picker.ModelPicker`` + ``ModelEntry``,
``widgets.progress_bar.IngestionProgress``, ``widgets.notifications.Notice``.

**Registration.** The stack is built from ``VIEW_SPECS`` below: view key → module path →
class name. A view module that does not import yet is replaced by a labelled placeholder
and logged, so the app runs throughout the build. ``SettingsView`` must expose
``show_tab(tab: str)`` for ``ctx.navigate("settings", "providers")`` to land correctly.

Never import this module from a view — import ``private_ai.ui.context`` instead.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from importlib import import_module
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QSettings, Qt, QTimer
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui import icons, theme
from private_ai.ui.async_bridge import set_toast_sink
from private_ai.ui.context import AppContext
from private_ai.ui.format import status_label
from private_ai.ui.widgets.notifications import Notice
from private_ai.ui.widgets.sidebar import NAVIGATION, Sidebar
from private_ai.ui.widgets.toast import ToastOverlay
from private_ai.ui.widgets.topbar import Topbar

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

logger = logging.getLogger("private_ai.ui.window")

__all__ = ["MainWindow", "VIEW_SPECS"]

GEOMETRY_KEY = "ui/window_geometry"
STATE_KEY = "ui/window_state"
SIDEBAR_KEY = "ui/sidebar_collapsed"

HEALTH_INTERVAL_MS = 5000
DOCUMENT_INTERVAL_MS = 2500


@dataclass(frozen=True)
class ViewSpec:
    key: str
    module: str
    klass: str
    label: str


VIEW_SPECS: tuple[ViewSpec, ...] = (
    ViewSpec("chat", "private_ai.ui.views.chat_view", "ChatView", "Trò chuyện"),
    ViewSpec("workspaces", "private_ai.ui.views.workspaces_view", "WorkspacesView", "Không gian"),
    ViewSpec("library", "private_ai.ui.views.library_view", "LibraryView", "Tài liệu"),
    ViewSpec("graph", "private_ai.ui.views.graph_view", "GraphView", "Tri thức"),
    ViewSpec("settings", "private_ai.ui.views.settings_view", "SettingsView", "Cài đặt"),
)


class _Placeholder(QWidget):
    """Stands in for a view that could not be imported.

    Deliberately loud rather than blank: during the build several views do not exist yet,
    and a screen that says why is worth more than an empty pane that looks like a bug.
    """

    def __init__(self, label: str, reason: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        gutter = theme.SPACE["4xl"]
        layout = QVBoxLayout(self)
        layout.setContentsMargins(gutter, gutter, gutter, gutter)
        layout.setSpacing(theme.SPACE["md"])
        layout.addStretch(1)
        mark = QLabel(self)
        mark.setPixmap(icons.pixmap("wrench", 28, theme.token("muted")))
        mark.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title = QLabel(f"Màn hình “{label}” chưa sẵn sàng", self)
        title.setProperty("class", "title")
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        # The import error is the whole point of this screen, so it reads at `muted`.
        detail = QLabel(reason, self)
        detail.setProperty("class", "muted")
        detail.setWordWrap(True)
        detail.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(mark)
        layout.addWidget(title)
        layout.addWidget(detail)
        layout.addStretch(1)


class MainWindow(QMainWindow):
    def __init__(self, services: AppServices, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle("Private AI")
        self.setMinimumSize(1040, 680)

        self.ctx = AppContext(services, window=self, parent=self)
        self.ctx.toastRequested.connect(self._on_toast)
        self.ctx.noticeRaised.connect(self._on_notice)
        self.ctx.navigateRequested.connect(self.show_view)
        self.ctx.workspaceChanged.connect(lambda _: self.refresh_sidebar())
        self.ctx.documentsChanged.connect(self._refresh_documents)

        self._views: dict[str, QWidget] = {}
        self._current_key = ""
        self._health_state: dict[str, str] = {}
        self._document_summary: dict[str, int] = {}
        self._first_busy_document: dict[str, Any] | None = None
        self._model_events: list[dict[str, Any]] = []
        self._raised: list[Notice] = []
        self._settings = QSettings("PrivateAI", "PrivateAI")

        self._build()
        self._restore_geometry()

        self.toasts = ToastOverlay(self)
        set_toast_sink(self._on_toast)

        self._health_timer = QTimer(self)
        self._health_timer.setInterval(HEALTH_INTERVAL_MS)
        self._health_timer.timeout.connect(self._poll_health)
        self._health_timer.start()

        self._document_timer = QTimer(self)
        self._document_timer.setInterval(DOCUMENT_INTERVAL_MS)
        self._document_timer.timeout.connect(self._poll_documents)
        self._document_timer.start()

        self.show_view("chat")
        self.sidebar.set_collapsed(bool(self._settings.value(SIDEBAR_KEY, False, type=bool)))
        # Deferred one tick: the qasync loop is set but not yet running while the window
        # is being constructed, and a task created before ``run_forever`` would step on a
        # loop that does not consider itself running.
        QTimer.singleShot(0, self._bootstrap)

    # ------------------------------------------------------------------ build
    def _build(self) -> None:
        central = QWidget(self)
        central.setObjectName("MainStage")
        row = QHBoxLayout(central)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(0)

        self.sidebar = Sidebar(self.ctx, central)
        self.sidebar.navigate.connect(self.show_view)
        self.sidebar.newConversation.connect(self._new_conversation)
        self.sidebar.workspaceChosen.connect(self._choose_workspace)
        self.sidebar.workspaceDeleted.connect(self._delete_workspace)
        self.sidebar.workspaceCreateRequested.connect(self._create_workspace)
        self.sidebar.conversationChosen.connect(self.ctx.set_conversation)
        self.sidebar.conversationDeleted.connect(self._delete_conversation)
        self.sidebar.collapsedChanged.connect(
            lambda value: self._settings.setValue(SIDEBAR_KEY, value)
        )
        row.addWidget(self.sidebar)

        stage = QWidget(central)
        stage_box = QVBoxLayout(stage)
        stage_box.setContentsMargins(0, 0, 0, 0)
        stage_box.setSpacing(0)

        self.topbar = Topbar(stage)
        self.topbar.notificationsOpened.connect(self._poll_health)
        stage_box.addWidget(self.topbar)

        self.stack = QStackedWidget(stage)
        stage_box.addWidget(self.stack, 1)
        row.addWidget(stage, 1)
        self.setCentralWidget(central)

        for spec in VIEW_SPECS:
            self._views[spec.key] = self._load_view(spec)
            self.stack.addWidget(self._views[spec.key])

    def _load_view(self, spec: ViewSpec) -> QWidget:
        try:
            module = import_module(spec.module)
            factory = getattr(module, spec.klass)
        except Exception as error:  # noqa: BLE001 - a missing view must not kill the app
            logger.warning("Chưa nạp được màn hình %s (%s): %s", spec.key, spec.module, error)
            return _Placeholder(spec.label, f"{spec.module}.{spec.klass}: {error}", self.stack)
        try:
            return factory(self.ctx, self.stack)
        except Exception as error:  # noqa: BLE001 - a broken constructor, likewise
            logger.exception("Màn hình %s dựng lỗi", spec.key)
            return _Placeholder(spec.label, f"{spec.klass}: {error}", self.stack)

    # ------------------------------------------------------------------- nav
    def show_view(self, key: str, sub_tab: str = "") -> None:
        widget = self._views.get(key)
        if widget is None:
            return
        if key != self._current_key:
            previous = self._views.get(self._current_key)
            hook = getattr(previous, "on_deactivated", None)
            if callable(hook):
                self._safely(hook)
            self._current_key = key
            self.stack.setCurrentWidget(widget)
            self.sidebar.set_active_view(key)
            hook = getattr(widget, "on_activated", None)
            if callable(hook):
                self._safely(hook)
        if sub_tab:
            show_tab = getattr(widget, "show_tab", None)
            if callable(show_tab):
                self._safely(lambda: show_tab(sub_tab))
        self.topbar.set_context_visible(key == "chat")
        self._refresh_title()

    @staticmethod
    def _safely(call) -> None:
        try:
            call()
        except Exception:  # noqa: BLE001 - a view hook must not break navigation
            logger.exception("Lỗi trong vòng đời màn hình")

    def _refresh_title(self) -> None:
        labels = {key: label for key, label, _ in NAVIGATION}
        if self._current_key == "chat":
            self.topbar.set_title(self._workspace_name or "Chưa có không gian")
        else:
            self.topbar.set_title(labels.get(self._current_key, "Private AI"))

    # ------------------------------------------------------------- data flow
    _workspace_name = ""

    def _bootstrap(self) -> None:
        self.sidebar.refresh_profiles()
        self.refresh_sidebar(select_first=True)
        self._poll_health()

    def refresh_sidebar(self, *, select_first: bool = False) -> None:
        def loaded(workspaces) -> None:
            items = list(workspaces)
            if select_first and not self.ctx.workspace_id and items:
                self.ctx.set_workspace(str(items[0].id))
            current = next((w for w in items if str(w.id) == self.ctx.workspace_id), None)
            self._workspace_name = str(getattr(current, "name", "") or "")
            self.sidebar.set_workspaces(items, self.ctx.workspace_id)
            self._refresh_title()
            self._refresh_conversations()
            self._refresh_documents()

        self.ctx.run(repositories.list_workspaces(self.ctx.database), on_result=loaded)

    def _refresh_conversations(self) -> None:
        if not self.ctx.workspace_id:
            self.sidebar.set_conversations([], "")
            return
        self.ctx.run(
            repositories.list_conversations(self.ctx.database, self.ctx.workspace_id),
            on_result=lambda items: self.sidebar.set_conversations(
                list(items), self.ctx.conversation_id
            ),
        )

    # ----------------------------------------------------------- sidebar acts
    def _choose_workspace(self, workspace_id: str) -> None:
        self.ctx.set_workspace(workspace_id)
        self.show_view("chat")

    def _create_workspace(self) -> None:
        # The dialog belongs to another agent's module; fall back to the workspaces
        # screen so the action is never a dead click while that lands.
        try:
            from private_ai.ui.dialogs.workspace_dialog import WorkspaceDialog
        except Exception:  # noqa: BLE001 - not built yet
            self.show_view("workspaces")
            return
        dialog = WorkspaceDialog(self.ctx, parent=self)
        if hasattr(dialog, "saved"):
            dialog.saved.connect(lambda *_: self.refresh_sidebar())
        dialog.open()

    def _delete_workspace(self, workspace_id: str) -> None:
        def done(_result) -> None:
            if workspace_id == self.ctx.workspace_id:
                self.ctx.set_workspace("")
            self.ctx.toast("Đã xóa không gian làm việc", "success")
            self.refresh_sidebar(select_first=True)

        self.ctx.run(
            repositories.delete_workspace(self.ctx.database, workspace_id, confirmed=True),
            on_result=done,
            on_error=lambda error: self.ctx.toast(f"Không xóa được không gian: {error}", "error"),
        )

    def _new_conversation(self) -> None:
        self.show_view("chat")
        if not self.ctx.workspace_id:
            self.ctx.toast("Hãy tạo một không gian làm việc trước", "info")
            return
        # An empty conversation is created lazily by the chat view on first send; here we
        # only clear the selection so the composer starts fresh.
        self.ctx.set_conversation("")
        self._refresh_conversations()

    def _delete_conversation(self, conversation_id: str) -> None:
        def done(_result) -> None:
            if conversation_id == self.ctx.conversation_id:
                self.ctx.set_conversation("")
            self.ctx.toast("Đã xóa cuộc trò chuyện", "success")
            self._refresh_conversations()

        self.ctx.run(
            repositories.delete_conversation(self.ctx.database, conversation_id, confirmed=True),
            on_result=done,
            on_error=lambda error: self.ctx.toast(
                f"Không xóa được cuộc trò chuyện: {error}", "error"
            ),
        )

    # ----------------------------------------------------------------- health
    def _poll_health(self) -> None:
        if not self.isVisible():
            return
        self.ctx.run(self._read_health(), on_result=self._apply_health, on_error=lambda _: None)

    async def _read_health(self) -> dict[str, str]:
        services = self.ctx.services
        state: dict[str, str] = {}
        config = services.providers.active_config()
        if config is None:
            state["provider"] = "not_configured"
            state["local_runtime"] = "not_configured"
        else:
            ok = await services.models.health()
            state["provider"] = "online" if ok else "offline"
            state["on_device"] = "1" if config.on_device else "0"
            state["local_runtime"] = "online" if ok or not config.on_device else "offline"
        state["knowledge_graph"] = "online" if await services.graph.health() else "not_configured"
        state["asr"] = "online" if await services.asr.health() else "offline"
        try:
            self._model_events = list(await repositories.list_model_events(self.ctx.database, 20))
        except Exception:  # noqa: BLE001 - events are advisory
            self._model_events = []
        return state

    def _apply_health(self, state: dict[str, str]) -> None:
        self._health_state = state
        online = state.get("provider") == "online"
        self.topbar.set_status("online" if online else "offline")
        self.sidebar.set_online(online)
        self._rebuild_notices()

    def _refresh_documents(self) -> None:
        if not self.ctx.workspace_id:
            self._document_summary = {}
            self._first_busy_document = None
            self._rebuild_notices()
            return

        def loaded(page: dict[str, Any]) -> None:
            summary = page.get("summary") or {}
            self._document_summary = {
                key: int(summary.get(key, 0) or 0) for key in ("pending", "indexing", "failed")
            }
            items = list(page.get("items") or [])
            self._first_busy_document = next(
                (item for item in items if str(item.get("status")) in ("queued", "processing")),
                None,
            )
            self._first_failed_document = next(
                (item for item in items if str(item.get("status")) in ("failed", "needs_ocr")),
                None,
            )
            self._rebuild_notices()

        self.ctx.run(
            repositories.list_documents(self.ctx.database, self.ctx.workspace_id, limit=20),
            on_result=loaded,
            on_error=lambda _: None,
        )

    _first_failed_document: dict[str, Any] | None = None

    def _poll_documents(self) -> None:
        # Gated on visibility and on there actually being work: a workspace with nothing
        # in flight should not wake the database every 2.5 seconds.
        if not self.isVisible():
            return
        working = self._document_summary.get("pending", 0) or self._document_summary.get(
            "indexing", 0
        )
        if not working:
            return
        self._refresh_documents()

    # --------------------------------------------------------------- notices
    def _rebuild_notices(self) -> None:
        """Derived state, exactly as ``App.tsx`` did it: health first, then documents,
        then the last few failed model events."""
        notices: list[Notice] = []
        state = self._health_state.get

        def to(view: str, tab: str = "") -> Any:
            return lambda: self.ctx.navigate(view, tab)

        if state("provider") == "not_configured":
            notices.append(
                Notice(
                    "provider-missing",
                    "Chưa chọn nhà cung cấp AI",
                    "warn",
                    "Thêm Ollama hoặc một endpoint tương thích OpenAI trong Cài đặt.",
                    action_label="Mở nhà cung cấp",
                    action=to("settings", "providers"),
                )
            )
        elif state("provider") == "offline":
            notices.append(
                Notice(
                    "provider-offline",
                    "Nhà cung cấp AI không phản hồi",
                    "alert",
                    "Không gọi được endpoint đang chọn, cuộc trò chuyện sẽ lỗi.",
                    action_label="Kiểm tra nhà cung cấp",
                    action=to("settings", "providers"),
                )
            )
        if state("on_device") == "1" and state("local_runtime") == "offline":
            notices.append(
                Notice(
                    "local-runtime-offline",
                    "Máy chủ mô hình cục bộ đã ngoại tuyến",
                    "alert",
                    "Nhà cung cấp đang chọn chạy trên máy này nhưng runtime không phản hồi.",
                    action_label="Mở mô hình",
                    action=to("settings", "models"),
                )
            )
        if state("knowledge_graph") == "not_configured":
            notices.append(
                Notice(
                    "graph-missing",
                    "Kho tri thức chưa dựng",
                    "warn",
                    "Tải tài liệu lên để Private AI lập chỉ mục và trả lời theo ngữ cảnh.",
                    action_label="Mở tài liệu",
                    action=to("library"),
                )
            )
        if state("asr") == "offline":
            notices.append(
                Notice(
                    "asr-offline",
                    "Nhập bằng giọng nói chưa sẵn sàng",
                    "warn",
                    "Cài mô hình nhận dạng giọng nói trong Cài đặt → Mô hình.",
                    action_label="Mở mô hình",
                    action=to("settings", "models"),
                )
            )

        failed = self._document_summary.get("failed", 0)
        if failed:
            first = self._first_failed_document or {}
            detail = (
                f"{first.get('filename')}: "
                f"{first.get('error') or status_label(str(first.get('status', '')))}"
                if first
                else "Mở Thư viện để xem nguyên nhân và thử xử lý lại."
            )
            notices.append(
                Notice(
                    "documents-failed",
                    f"{failed} tài liệu xử lý lỗi",
                    "alert",
                    detail,
                    action_label="Mở tài liệu",
                    action=to("library"),
                )
            )
        pending = self._document_summary.get("pending", 0)
        if pending:
            first = self._first_busy_document or {}
            notices.append(
                Notice(
                    "documents-pending",
                    f"{pending} tài liệu đang xử lý",
                    "info",
                    f"{first.get('filename')}: {status_label(str(first.get('status', '')))}"
                    if first
                    else "Nội dung sẽ vào kho tri thức khi trích xuất xong.",
                    action_label="Theo dõi tài liệu",
                    action=to("library"),
                )
            )
        indexing = self._document_summary.get("indexing", 0)
        if indexing:
            notices.append(
                Notice(
                    "documents-indexing",
                    f"{indexing} tài liệu đang vào kho tri thức",
                    "info",
                    "Đang tạo embedding và graph memory.",
                    action_label="Theo dõi tài liệu",
                    action=to("library"),
                )
            )

        for event in [e for e in self._model_events if str(e.get("status")) == "failed"][:6]:
            notices.append(
                Notice(
                    str(event.get("id", "")),
                    f"{event.get('action', 'Thao tác mô hình')} thất bại",
                    "alert",
                    f"{event.get('model_name', '')} — {event.get('detail', '')}".strip(" —"),
                    at=str(event.get("created_at", "") or ""),
                    action_label="Mở mô hình",
                    action=to("settings", "models"),
                )
            )

        notices.extend(self._raised)
        self.topbar.set_notices(notices)

    def _on_notice(self, notice: Notice) -> None:
        self._raised = [item for item in self._raised if item.id != notice.id][-19:]
        self._raised.append(notice)
        self._rebuild_notices()

    def _on_toast(self, message: str, tone: str = "info") -> None:
        self.toasts.show_toast(message, tone)

    # --------------------------------------------------------------- geometry
    def _restore_geometry(self) -> None:
        geometry = self._settings.value(GEOMETRY_KEY)
        if geometry is not None:
            self.restoreGeometry(geometry)
        else:
            self.resize(1360, 860)
        state = self._settings.value(STATE_KEY)
        if state is not None:
            self.restoreState(state)

    def closeEvent(self, event) -> None:  # noqa: N802 - Qt override
        # Geometry is the one preference that stays in QSettings: it is per-display, and
        # syncing it through the database would move the window on another machine.
        self._settings.setValue(GEOMETRY_KEY, self.saveGeometry())
        self._settings.setValue(STATE_KEY, self.saveState())
        self._health_timer.stop()
        self._document_timer.stop()
        for view in self._views.values():
            hook = getattr(view, "on_deactivated", None)
            if callable(hook):
                self._safely(hook)
        super().closeEvent(event)
