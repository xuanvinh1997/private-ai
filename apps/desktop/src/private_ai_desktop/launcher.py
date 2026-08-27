from __future__ import annotations

import os
from pathlib import Path

import webview

from private_ai_desktop.runtime import RuntimeController


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


def main() -> None:
    runtime = RuntimeController(mode=os.getenv("PRIVATE_AI_DESKTOP_RUNTIME", "auto"))
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
    try:
        webview.start(debug=os.getenv("PRIVATE_AI_DEBUG") == "1")
    finally:
        runtime.stop()


if __name__ == "__main__":
    main()
