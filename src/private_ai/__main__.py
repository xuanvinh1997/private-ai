"""``python -m private_ai`` — the desktop app.

The UI is the only part of the tree that may import Qt, so it is imported here and not
at module scope: the worker and the MCP servers reach the package without PySide6 on
their path and must not pay for it.
"""

from __future__ import annotations

import sys


def main() -> int:
    try:
        from private_ai.ui.app import main as run_app
    except ImportError as exc:
        missing = getattr(exc, "name", "") or ""
        if missing.split(".")[0] == "PySide6":
            print(
                "Không tìm thấy PySide6. Cài giao diện bằng:\n"
                "    pip install 'private-ai[dev]'  hoặc  pip install PySide6 qasync",
                file=sys.stderr,
            )
        else:
            print(f"Không khởi động được giao diện: {exc}", file=sys.stderr)
        return 1
    return run_app() or 0


if __name__ == "__main__":
    raise SystemExit(main())
