from __future__ import annotations

import os
import socket
import sys

import pytest

from private_ai_desktop.runtime import RuntimeController, port_is_taken, workspace_root


def test_command_always_starts_local_api() -> None:
    controller = RuntimeController()
    command, cwd, _ = controller.command()

    assert command == [
        sys.executable,
        "-m",
        "uvicorn",
        "private_ai_api.main:app",
        "--host",
        "127.0.0.1",
        "--port",
        "8000",
    ]
    assert cwd is not None
    assert (cwd / "services" / "api" / "src").is_dir()


def test_command_follows_the_configured_host_and_port(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PRIVATE_AI_PORT", "8123")
    controller = RuntimeController()

    command, _, _ = controller.command()

    assert controller.api_url == "http://127.0.0.1:8123"
    assert command[-1] == "8123"


def test_command_puts_the_api_source_first_on_the_python_path() -> None:
    _, cwd, environment = RuntimeController().command()

    assert cwd is not None
    first = environment["PYTHONPATH"].split(os.pathsep)[0]
    assert first == str(cwd / "services" / "api" / "src")


def test_start_refuses_a_port_held_by_another_application(monkeypatch: pytest.MonkeyPatch) -> None:
    with socket.socket() as squatter:
        squatter.bind(("127.0.0.1", 0))
        squatter.listen(16)  # is_ready() probes first, and closed sockets linger in the queue
        controller = RuntimeController(port=squatter.getsockname()[1])
        monkeypatch.setattr(RuntimeController, "preflight", lambda self: None)

        with pytest.raises(RuntimeError, match="held by another application"):
            controller.start(timeout_seconds=1.0)

    assert controller.process is None


def test_preflight_requires_a_built_frontend(monkeypatch: pytest.MonkeyPatch, tmp_path) -> None:
    monkeypatch.setenv("PRIVATE_AI_FRONTEND_DIST", str(tmp_path / "missing"))

    with pytest.raises(RuntimeError, match="has not been built"):
        RuntimeController().preflight()


def test_port_is_taken_reports_a_free_port() -> None:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        free_port = probe.getsockname()[1]

    assert port_is_taken("127.0.0.1", free_port) is False


def test_workspace_root_honours_the_configured_project_dir(
    monkeypatch: pytest.MonkeyPatch, tmp_path
) -> None:
    monkeypatch.setenv("PRIVATE_AI_PROJECT_DIR", str(tmp_path))

    assert workspace_root() == tmp_path.resolve()
