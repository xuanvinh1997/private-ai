"""Process entry point: one QApplication, one asyncio loop, one service container.

``qasync`` runs the asyncio loop *inside* Qt's, so there is exactly one loop and no
thread hop between a button press and a service call. Everything downstream depends on
that: ``AgentRunner.stream`` is an async generator consumed straight from a Qt slot, and
the ASR stream is fed from the audio callback without a queue in between.

Startup is deliberately forgiving. A machine with no provider configured, a dead Ollama,
or a graph store that cannot open must still reach the Settings screen — refusing to
start is how a user ends up with an app they cannot fix.
"""

from __future__ import annotations

import asyncio
import logging
import signal
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

logger = logging.getLogger("private_ai.ui.app")

__all__ = ["main"]


def _configure_logging() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)-7s %(name)s: %(message)s",
    )
    # Qt's own chatter is noisy on Wayland and macOS and says nothing we act on.
    logging.getLogger("qasync").setLevel(logging.WARNING)


def _build_application():
    from PySide6.QtGui import QGuiApplication
    from PySide6.QtWidgets import QApplication

    QGuiApplication.setDesktopFileName("private-ai")
    app = QApplication.instance() or QApplication(sys.argv)
    app.setApplicationName("Private AI")
    app.setApplicationDisplayName("Private AI")
    app.setOrganizationName("PrivateAI")
    app.setOrganizationDomain("private-ai.local")
    # The last window closing is the quit signal; without this the loop keeps running
    # after the user closes the window and the process never exits.
    app.setQuitOnLastWindowClosed(True)
    return app


async def _startup(services: AppServices) -> str:
    """Bring up the loop-bound half of the services, reporting rather than raising.

    Returns a Vietnamese warning to show as a toast, or "" when everything came up.
    """
    from private_ai.core.bootstrap import start_services

    try:
        await start_services(services)
    except Exception as error:  # noqa: BLE001 - a broken MCP server must not block the UI
        logger.exception("Không khởi động được đầy đủ dịch vụ")
        return f"Một số dịch vụ chưa khởi động được: {error}"
    if services.providers.active_config() is None:
        return "Chưa cấu hình nhà cung cấp AI. Mở Cài đặt → Nhà cung cấp để thêm một cái."
    return ""


def main() -> int:
    _configure_logging()

    import qasync

    from private_ai.core.bootstrap import build_services, close_services
    from private_ai.ui import theme
    from private_ai.ui.async_bridge import cancel_all
    from private_ai.ui.main_window import MainWindow

    app = _build_application()
    loop = qasync.QEventLoop(app)
    asyncio.set_event_loop(loop)

    try:
        services = build_services()
    except Exception as error:  # noqa: BLE001 - nothing to show a dialog *in* yet
        logger.exception("Không mở được kho dữ liệu")
        from PySide6.QtWidgets import QMessageBox

        QMessageBox.critical(None, "Private AI", f"Không mở được kho dữ liệu:\n{error}")
        return 1

    # Preferences before the window: the first paint should already be the right theme.
    from private_ai.core.preferences import read_app_preferences

    preferences = read_app_preferences(services.database)
    theme.apply_theme(app, preferences.ui_theme, preferences.ui_font_scale)

    window = MainWindow(services)
    window.show()

    warning = ""
    closing = asyncio.Event()

    def request_quit(*_: object) -> None:
        closing.set()

    app.aboutToQuit.connect(request_quit)
    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, request_quit)
        except (NotImplementedError, AttributeError, ValueError):  # pragma: no cover - Windows
            signal.signal(sig, lambda *_: request_quit())

    async def run() -> None:
        nonlocal warning
        warning = await _startup(services)
        if warning:
            window.ctx.toast(warning, "error")
        # The shell can only fill its lists once the services behind them are up.
        window.refresh_sidebar(select_first=True)
        window.ctx.refresh_preferences()
        await closing.wait()

    with loop:
        try:
            loop.run_until_complete(run())
        except (KeyboardInterrupt, asyncio.CancelledError):  # pragma: no cover
            pass
        finally:
            cancel_all()
            try:
                loop.run_until_complete(close_services(services))
            except Exception:  # noqa: BLE001 - teardown must not mask the exit reason
                logger.exception("Lỗi khi đóng dịch vụ")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
