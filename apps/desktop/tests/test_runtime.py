from __future__ import annotations

import contextlib
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


def process_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def test_containment_gives_the_api_its_own_group() -> None:
    containment = RuntimeController._containment()

    if os.name == "nt":
        assert "creationflags" in containment
    else:
        assert containment == {"start_new_session": True}


def test_stop_kills_the_children_the_api_started(tmp_path) -> None:
    """The API starts FFmpeg and the transcriber, and those must not outlive the window.

    Terminating uvicorn alone leaves them running, so this spawns a stand-in that has a child
    of its own and asserts the grandchild is gone once the controller stops.
    """
    import subprocess
    import time

    marker = tmp_path / "grandchild.pid"
    script = tmp_path / "fake_api.py"
    script.write_text(
        "import pathlib, subprocess, sys, time\n"
        "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'])\n"
        "pathlib.Path(sys.argv[1]).write_text(str(child.pid))\n"
        "time.sleep(120)\n",
        encoding="utf-8",
    )

    controller = RuntimeController()
    controller.process = subprocess.Popen(  # noqa: S603
        [sys.executable, str(script), str(marker)],
        **RuntimeController._containment(),
    )
    controller._contain(controller.process)
    try:
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and not marker.exists():
            time.sleep(0.05)
        assert marker.exists(), "the stand-in never reported its child"
        grandchild = int(marker.read_text())
        assert process_alive(grandchild)

        controller.stop()

        assert controller.process is None
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and process_alive(grandchild):
            time.sleep(0.05)
        assert not process_alive(grandchild), "the API's child outlived the window"
    finally:
        with contextlib.suppress(Exception):
            controller.stop()


def test_stop_is_safe_to_call_twice_and_without_a_start() -> None:
    controller = RuntimeController()

    controller.stop()
    controller.stop()

    assert controller.process is None
