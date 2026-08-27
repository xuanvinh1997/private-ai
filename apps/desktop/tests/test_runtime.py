from __future__ import annotations

import pytest

from private_ai_desktop.runtime import RuntimeController


def test_auto_runtime_is_local_off_windows(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("platform.system", lambda: "Darwin")
    assert RuntimeController().resolved_mode() == "local"


def test_wsl_command_uses_argument_list_without_shell(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("platform.system", lambda: "Windows")
    monkeypatch.setenv("PRIVATE_AI_WSL_DISTRO", "Ubuntu-24.04")
    monkeypatch.setenv("PRIVATE_AI_WSL_PROJECT_DIR", "/opt/private-ai")

    command, cwd, _ = RuntimeController(mode="wsl").command()

    assert command == [
        "wsl.exe",
        "--distribution",
        "Ubuntu-24.04",
        "--cd",
        "/opt/private-ai",
        "--",
        "/opt/private-ai/.venv/bin/private-ai-api",
    ]
    assert cwd is None


def test_invalid_runtime_mode_is_rejected(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PRIVATE_AI_DESKTOP_RUNTIME", raising=False)
    with pytest.raises(ValueError):
        RuntimeController(mode="container").resolved_mode()
