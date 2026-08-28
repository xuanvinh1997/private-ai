from __future__ import annotations

import atexit
import os
import platform
import signal
import sys
from contextlib import suppress
from pathlib import Path

import webview

from private_ai_desktop.runtime import RuntimeController

WEBVIEW2_DOWNLOAD = "https://developer.microsoft.com/microsoft-edge/webview2/"


class DesktopApi:
    def choose_document(self) -> str | None:
        window = webview.windows[0]
        result = window.create_file_dialog(
            webview.FileDialog.OPEN,
            allow_multiple=False,
            file_types=("Documents (*.pdf;*.docx;*.pptx;*.xlsx;*.md;*.txt)", "All files (*.*)"),
        )
        if not result:
            return None
        selected = result[0] if isinstance(result, tuple | list) else result
        return str(Path(selected).resolve())


def ensure_web_engine() -> None:
    """Without WebView2, pywebview falls back to MSHTML, which cannot run the UI."""
    if platform.system() != "Windows":
        return
    try:
        from webview.platforms import winforms
    except ImportError:  # pragma: no cover - resolved by pywebview's own dependencies
        return
    if getattr(winforms, "renderer", "") == "mshtml":
        raise RuntimeError(
            "Windows is falling back to the Internet Explorer engine because the Microsoft Edge "
            f"WebView2 runtime is missing, and the Private AI interface cannot run on it. "
            f"Install the Evergreen runtime from {WEBVIEW2_DOWNLOAD} and start the app again."
        )


def report(error: Exception) -> None:
    message = str(error)
    print(f"Private AI could not start.\n\n{message}", file=sys.stderr)
    if platform.system() == "Windows":
        import ctypes

        ctypes.windll.user32.MessageBoxW(None, message, "Private AI", 0x10)


def install_signal_handlers(runtime: RuntimeController) -> None:
    """Stop the API on a signal too: a plain SIGTERM never unwinds to the finally block."""

    def handle(number: int, frame: object) -> None:
        runtime.stop()
        # Re-raise with the default action so the exit status still reports the signal.
        signal.signal(number, signal.SIG_DFL)
        os.kill(os.getpid(), number)

    for number in (signal.SIGINT, signal.SIGTERM):
        with suppress(AttributeError, OSError, ValueError):
            signal.signal(number, handle)


def main() -> None:
    runtime = RuntimeController()
    # Three independent paths, because no single one covers every way the window goes away:
    # the closed event for a normal close, signals for a shutdown or a kill, and atexit for
    # anything that unwinds the interpreter without passing through the finally block.
    atexit.register(runtime.stop)
    install_signal_handlers(runtime)
    try:
        ensure_web_engine()
        runtime.start()
        window = webview.create_window(
            "Private AI",
            runtime.api_url,
            js_api=DesktopApi(),
            width=1440,
            height=920,
            min_size=(1024, 700),
            background_color="#f3f6f4",
        )
        # Closing the last window ends webview.start(), but the API is stopped here so a
        # slow or wedged GUI teardown cannot leave it serving.
        with suppress(AttributeError):
            window.events.closed += runtime.stop
        webview.start(debug=os.getenv("PRIVATE_AI_DEBUG") == "1")
    except (RuntimeError, TimeoutError) as error:
        report(error)
        raise SystemExit(1) from error
    finally:
        runtime.stop()


if __name__ == "__main__":
    main()
