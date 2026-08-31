"""What every view is handed: the services, the current selection, and a way to react.

Kept out of ``main_window`` on purpose. Views import this; the window imports the views.
If ``AppContext`` lived in the window module every view would import the window and the
whole UI package would be one import cycle.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QObject, Signal

from private_ai.core import preferences as prefs
from private_ai.core.preferences import AppPreferences, read_app_preferences
from private_ai.ui import theme
from private_ai.ui.async_bridge import run_coro
from private_ai.ui.format import TONES

if TYPE_CHECKING:  # pragma: no cover - import graph only
    import asyncio
    from collections.abc import Callable, Coroutine

    from PySide6.QtWidgets import QMainWindow

    from private_ai.config import Settings
    from private_ai.core.database import Database
    from private_ai.core.services import AppServices
    from private_ai.ui.widgets.notifications import Notice

logger = logging.getLogger("private_ai.ui.context")

__all__ = ["AppContext"]


class AppContext(QObject):
    """The shell's shared state. One instance, owned by ``MainWindow``."""

    workspaceChanged = Signal(str)
    conversationChanged = Signal(str)
    themeChanged = Signal(str)
    preferencesChanged = Signal(object)
    documentsChanged = Signal()
    modelsChanged = Signal()
    navigateRequested = Signal(str, str)
    noticeRaised = Signal(object)
    toastRequested = Signal(str, str)

    def __init__(
        self,
        services: AppServices,
        *,
        window: QMainWindow | None = None,
        parent: QObject | None = None,
    ) -> None:
        super().__init__(parent)
        self.services = services
        self.window = window
        self.preferences: AppPreferences = read_app_preferences(services.database)
        self.workspace_id: str = ""
        self.conversation_id: str = ""
        self.theme_name: str = theme.resolve_theme_name(self.preferences.ui_theme)
        self.font_scale: str = self.preferences.ui_font_scale

    # ---------------------------------------------------------------- accessors
    @property
    def database(self) -> Database:
        return self.services.database

    @property
    def settings(self) -> Settings:
        return self.services.settings

    def tokens(self) -> dict[str, str]:
        return theme.tokens(self.theme_name)

    # ---------------------------------------------------------------- selection
    def set_workspace(self, workspace_id: str) -> None:
        value = workspace_id or ""
        if value == self.workspace_id:
            return
        self.workspace_id = value
        # A conversation belongs to exactly one workspace, so it cannot survive the swap.
        if self.conversation_id:
            self.conversation_id = ""
            self.conversationChanged.emit("")
        self.workspaceChanged.emit(value)

    def set_conversation(self, conversation_id: str) -> None:
        value = conversation_id or ""
        if value == self.conversation_id:
            return
        self.conversation_id = value
        self.conversationChanged.emit(value)

    # ------------------------------------------------------------- preferences
    def set_theme(self, name: str) -> None:
        resolved = theme.resolve_theme_name(name)
        self._persist(prefs.UI_THEME_KEY, name if name in prefs.UI_THEMES else resolved)
        if resolved == self.theme_name:
            return
        self.theme_name = resolved
        self._apply_theme()
        self.themeChanged.emit(resolved)

    def set_font_scale(self, scale: str) -> None:
        value = scale if scale in prefs.UI_FONT_SCALES else "normal"
        self._persist(prefs.UI_FONT_SCALE_KEY, value)
        if value == self.font_scale:
            return
        self.font_scale = value
        self._apply_theme()
        # The scale changes metrics everywhere, so listeners re-measure on the same signal.
        self.themeChanged.emit(self.theme_name)

    def _apply_theme(self) -> None:
        from PySide6.QtWidgets import QApplication

        app = QApplication.instance()
        if app is not None:
            theme.apply_theme(app, self.theme_name, self.font_scale)

    def _persist(self, key: str, value: str) -> None:
        # Optimistic: the UI has already switched. A failed write is worth a toast but
        # not worth reverting a change the user is looking at.
        self.run(
            prefs.write_app_preference_async(self.database, key, value),
            on_error=lambda error: self.toast(f"Không lưu được tùy chọn: {error}", "error"),
        )

    def refresh_preferences(self) -> None:
        def done(value: AppPreferences) -> None:
            self.preferences = value
            self.preferencesChanged.emit(value)

        self.run(prefs.read_app_preferences_async(self.database), on_result=done)

    def update_preferences(self, **changes: Any) -> None:
        """Optimistic write with rollback: the view binds to ``preferences`` immediately
        and only learns about a failure if the database refuses."""
        from private_ai.core.schemas import PreferencesUpdate

        previous = self.preferences
        update = PreferencesUpdate(**changes)
        self.preferences = prefs.apply_update(previous, update)
        self.preferencesChanged.emit(self.preferences)

        def failed(error: BaseException) -> None:
            self.preferences = previous
            self.preferencesChanged.emit(previous)
            self.toast(f"Không lưu được tùy chọn: {error}", "error")

        def saved(value: AppPreferences) -> None:
            self.preferences = value
            self.preferencesChanged.emit(value)

        self.run(
            prefs.write_app_preferences(self.database, update),
            on_result=saved,
            on_error=failed,
        )

    # ------------------------------------------------------------------- shell
    def run(
        self,
        coro: Coroutine[Any, Any, Any],
        *,
        on_result: Callable[[Any], None] | None = None,
        on_error: Callable[[BaseException], None] | None = None,
        label: str = "",
    ) -> asyncio.Task[Any]:
        return run_coro(coro, on_result, on_error, owner=self, label=label)

    def toast(self, message: str, tone: str = "info") -> None:
        self.toastRequested.emit(message, tone if tone in TONES else "info")

    def navigate(self, view_key: str, sub_tab: str = "") -> None:
        self.navigateRequested.emit(view_key, sub_tab)

    def notify(self, notice: Notice) -> None:
        self.noticeRaised.emit(notice)
