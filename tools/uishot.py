"""Render one view, dialog or widget offscreen and write a PNG per theme.

A redesign that is only read in the diff is a redesign nobody has seen. This builds the
real widget against real services — on a throwaway data directory, so it never touches
``.local-data`` — paints it light and dark, and drops the two files side by side.

    python tools/uishot.py views.models_view.ModelsView
    python tools/uishot.py views.settings_view.GeneralSettings --size 1000x1200
    python tools/uishot.py widgets.topbar.TopBar --out /tmp/shots --themes dark

The target is a dotted path under ``private_ai.ui``. Anything whose constructor takes the
``AppContext`` works; a widget that takes nothing at all works too.
"""

from __future__ import annotations

import argparse
import asyncio
import importlib
import os
import sys
import tempfile
from datetime import UTC, datetime
from pathlib import Path
from uuid import uuid4

# Before any Qt import: the platform plugin is chosen at QApplication construction.
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

SETTLE_SECONDS = 0.6


def _services(data_dir: Path):
    from private_ai.config import Settings
    from private_ai.core.bootstrap import build_services

    settings = Settings(
        data_dir=data_dir,
        inline_ingestion=True,
        mcp_require_auth=False,
        mcp_external_servers="",
        skill_paths="",
        file_roots="",
        asr_enabled=False,
    )
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    services = build_services(settings)
    row = services.database.fetch_one("SELECT id FROM workspaces LIMIT 1")
    if row:
        return services, str(row["id"])
    identifier = str(uuid4())
    now = datetime.now(UTC).isoformat()
    services.database.execute(
        "INSERT INTO workspaces(id, name, description, created_at, updated_at) "
        "VALUES (?, ?, '', ?, ?)",
        (identifier, "Xem thử", now, now),
    )
    return services, identifier


def _target(path: str):
    module_path, class_name = path.rsplit(".", 1)
    module = importlib.import_module(f"private_ai.ui.{module_path}")
    return getattr(module, class_name)


def _build(factory, ctx):
    """Views take the context, ``MainWindow`` takes the services, a plain widget takes
    nothing. ``MainWindow`` is checked by name because handing it the context raised deep
    inside its first health poll and painted the AttributeError into the shot."""
    from private_ai.ui.main_window import MainWindow

    if isinstance(factory, type) and issubclass(factory, MainWindow):
        return factory(ctx.services)
    try:
        return factory(ctx)
    except TypeError:
        return factory()


async def _shoot(factory, ctx, themes: list[str], size: tuple[int, int], out: Path, stem: str):
    from PySide6.QtWidgets import QApplication

    from private_ai.ui import theme as theme_module

    app = QApplication.instance()
    for name in themes:
        theme_module.apply_theme(app, name, "normal")
        widget = _build(factory, ctx)
        widget.resize(*size)
        widget.show()
        # Views that load on activation rather than on construction — the library is one —
        # otherwise get photographed in the state no user ever sees.
        activate = getattr(widget, "on_activated", None)
        if callable(activate):
            activate()
        # ``await`` rather than ``processEvents``: qasync pumps Qt from inside the sleep,
        # and re-entering the loop by hand raises "cannot enter into task". The wait is
        # what gives every view that loads its rows through ``ctx.run`` something to paint.
        await asyncio.sleep(SETTLE_SECONDS)
        target = out / f"{stem}-{name}.png"
        widget.grab().save(str(target))
        widget.close()
        print(target)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "target", help="dotted path under private_ai.ui, e.g. views.chat_view.ChatView"
    )
    parser.add_argument("--out", default=tempfile.gettempdir(), help="directory for the PNGs")
    parser.add_argument("--size", default="1100x760", help="WxH of the render")
    parser.add_argument("--themes", default="light,dark", help="comma separated theme names")
    parser.add_argument("--data-dir", default="", help="data directory (default: a throwaway one)")
    args = parser.parse_args()

    width, _, height = args.size.partition("x")
    out = Path(args.out).expanduser()
    out.mkdir(parents=True, exist_ok=True)

    import qasync
    from PySide6.QtWidgets import QApplication

    from private_ai.ui.context import AppContext

    app = QApplication.instance() or QApplication([])
    loop = qasync.QEventLoop(app)
    asyncio.set_event_loop(loop)

    with tempfile.TemporaryDirectory() as scratch:
        data_dir = Path(args.data_dir).expanduser() if args.data_dir else Path(scratch) / "data"
        services, workspace_id = _services(data_dir)
        ctx = AppContext(services=services)
        ctx.workspace_id = workspace_id
        factory = _target(args.target)
        with loop:
            loop.run_until_complete(
                _shoot(
                    factory,
                    ctx,
                    [name.strip() for name in args.themes.split(",") if name.strip()],
                    (int(width), int(height)),
                    out,
                    args.target.rsplit(".", 1)[-1].lower(),
                )
            )
        services.database.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
