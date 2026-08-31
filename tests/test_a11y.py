"""A glyph is not a name.

A button whose whole caption is an icon is announced by a screen reader as "button" and
nothing more: the tooltip is painted, never spoken. Rule 9 of the design language asks for
both on every icon-only control, and the two halves drifted apart — fifteen controls
carried a tooltip and no accessible name before this file existed. Every view is built for
real and every caption-less button is asked what it is called.
"""

from __future__ import annotations

import pytest

VIEWS = (
    "library_view.LibraryView",
    "workspaces_view.WorkspacesView",
    "settings_view.SettingsView",
    "models_view.ModelsView",
    "mcp_view.McpView",
    "memory_view.MemoryView",
    "providers_view.ProvidersView",
    "skills_view.SkillsView",
    "chat_view.ChatView",
    "graph_view.GraphView",
)


def _load(path: str):
    import importlib

    module_name, class_name = path.split(".")
    module = importlib.import_module(f"private_ai.ui.views.{module_name}")
    return getattr(module, class_name)


def _unnamed(view) -> list[str]:
    """Every icon-only button that cannot say what it does."""
    from PySide6.QtWidgets import QAbstractButton, QLineEdit

    missing: list[str] = []
    for button in view.findChildren(QAbstractButton):
        if (button.text() or "").strip():
            continue
        if button.icon().isNull():
            continue
        # Qt builds the search and clear affordances inside a QLineEdit itself; they are
        # not ours to name, and the field's own placeholder carries the meaning.
        if isinstance(button.parent(), QLineEdit):
            continue
        tooltip = (button.toolTip() or "").strip()
        name = (button.accessibleName() or "").strip()
        if tooltip and name:
            continue
        missing.append(f"{type(button).__name__} tooltip={tooltip!r} accessibleName={name!r}")
    return missing


@pytest.fixture
def built(services, workspace_id, qapp):
    from private_ai.ui import theme
    from private_ai.ui.context import AppContext

    theme.apply_theme(qapp, "light", "normal")
    context = AppContext(services=services)
    context.workspace_id = workspace_id
    made = []

    def build(path: str):
        view = _load(path)(context)
        view.resize(1200, 820)
        view.show()
        qapp.processEvents()
        qapp.processEvents()
        made.append(view)
        return view

    yield build
    for view in made:
        view.close()


@pytest.mark.parametrize("path", VIEWS)
async def test_icon_only_buttons_are_named(built, path: str) -> None:
    # Async because the views schedule their first load on construction, and that needs a
    # running loop the way the real application has one.
    view = built(path)
    missing = _unnamed(view)
    if missing:
        pytest.fail(f"{path}: icon-only buttons with no spoken name:\n  " + "\n  ".join(missing))
