"""Where a packaged build keeps its state, and what the bundle has to carry.

None of this fails visibly. A frozen app that resolves ``.local-data`` relative to a
working directory of ``/`` does not crash on a developer's machine, because on a developer's
machine it is never frozen; it crashes on the first user's. And the two registries below
name their modules as strings and swallow ImportError by design, so a bundle missing them
starts up looking fine — with placeholder screens and no tools. Both are asserted here
because neither shows up in a normal test run of the application itself.
"""

from __future__ import annotations

import sys
from importlib import import_module
from pathlib import Path

import pytest

from private_ai.config import BUNDLED_DATA_FOLDER, Settings, default_data_dir
from private_ai.mcp.client import BUILTIN_SERVERS
from private_ai.ui.main_window import VIEW_SPECS


def test_a_source_checkout_keeps_its_state_beside_the_source() -> None:
    assert default_data_dir() == Path(".local-data")


def test_a_frozen_build_resolves_under_the_home_directory(monkeypatch: pytest.MonkeyPatch) -> None:
    """``.local-data`` is relative, and a bundled app is launched with cwd set to ``/``."""
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    resolved = default_data_dir()
    assert resolved == Path.home() / BUNDLED_DATA_FOLDER
    assert resolved.is_absolute()


def test_the_bundled_path_is_the_same_name_on_every_platform(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """One path everywhere is the decision; a per-platform convention was the alternative."""
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    for platform in ("darwin", "win32", "linux"):
        monkeypatch.setattr(sys, "platform", platform)
        assert default_data_dir().name == ".private-ai"


def test_the_environment_still_wins_over_both(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setenv("PRIVATE_AI_DATA_DIR", str(tmp_path / "elsewhere"))
    assert Settings().data_dir == tmp_path / "elsewhere"


@pytest.mark.parametrize("module_path", sorted(BUILTIN_SERVERS))
def test_every_mcp_server_named_by_string_really_imports(module_path: str) -> None:
    """``McpHub._mount`` logs and skips a server that will not import, so a bundle that
    dropped one publishes no tools and says nothing a user would read as fatal."""
    assert hasattr(import_module(module_path), "create_server")


@pytest.mark.parametrize("spec", VIEW_SPECS, ids=lambda spec: spec.key)
def test_every_view_named_by_string_really_imports(spec: object) -> None:
    """Same shape of failure on the UI side: a missing view becomes a placeholder."""
    assert hasattr(import_module(spec.module), spec.klass)
