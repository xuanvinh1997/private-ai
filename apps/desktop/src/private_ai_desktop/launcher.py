from __future__ import annotations

import os
import platform
import sys
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


def main() -> None:
    runtime = RuntimeController()
    try:
        ensure_web_engine()
        runtime.start()
        webview.create_window(
            "Private AI",
            runtime.api_url,
            js_api=DesktopApi(),
            width=1440,
            height=920,
            min_size=(1024, 700),
            background_color="#f3f6f4",
        )
        webview.start(debug=os.getenv("PRIVATE_AI_DEBUG") == "1")
    except (RuntimeError, TimeoutError) as error:
        report(error)
        raise SystemExit(1) from error
    finally:
        runtime.stop()


if __name__ == "__main__":
    main()
