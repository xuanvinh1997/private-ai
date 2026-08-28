from __future__ import annotations

import os
import signal
import subprocess
import sys
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DESKTOP_SRC = ROOT / "apps" / "desktop" / "src"


def run_in_subprocess(body: str) -> subprocess.CompletedProcess[str]:
    """Run against a stubbed pywebview, so the shutdown paths are testable without a GUI."""
    script = textwrap.dedent(
        """
        import sys, types
        stub = types.ModuleType("webview")
        stub.windows = []
        stub.FileDialog = types.SimpleNamespace(OPEN=1)
        class Event(list):
            def __iadd__(self, handler):
                self.append(handler)
                return self
        class Events:
            def __init__(self): self.closed = Event()
        stub.subscribed = []
        def make_window(*a, **k):
            window = types.SimpleNamespace(events=Events())
            stub.subscribed.append(window)
            return window
        stub.create_window = make_window
        stub.start = lambda **k: None
        sys.modules["webview"] = stub
        sys.modules["webview.platforms"] = types.ModuleType("webview.platforms")
        """
    ) + textwrap.dedent(body)
    environment = os.environ.copy()
    environment["PYTHONPATH"] = os.pathsep.join(
        part for part in (str(DESKTOP_SRC), environment.get("PYTHONPATH")) if part
    )
    return subprocess.run(  # noqa: S603
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        timeout=60,
        env=environment,
        check=False,
    )


def test_sigterm_stops_the_api_before_the_launcher_dies() -> None:
    """A plain SIGTERM never unwinds to the finally block, so the handler has to do it."""
    result = run_in_subprocess(
        """
        import os, signal
        from private_ai_desktop.launcher import install_signal_handlers

        class Runtime:
            stopped = False
            def stop(self):
                Runtime.stopped = True
                print("STOPPED", flush=True)

        runtime = Runtime()
        install_signal_handlers(runtime)
        os.kill(os.getpid(), signal.SIGTERM)
        print("UNREACHABLE", flush=True)
        """
    )

    assert "STOPPED" in result.stdout
    assert "UNREACHABLE" not in result.stdout
    # The default action still runs, so the exit status keeps reporting the signal.
    assert result.returncode == -signal.SIGTERM


def test_the_api_is_stopped_when_the_window_closes() -> None:
    result = run_in_subprocess(
        """
        import private_ai_desktop.launcher as launcher

        calls = []

        class Runtime:
            api_url = "http://127.0.0.1:8000"
            def start(self): calls.append("start")
            def stop(self): calls.append("stop")

        launcher.RuntimeController = Runtime
        launcher.ensure_web_engine = lambda: None
        launcher.main()
        import webview
        window = webview.subscribed[0]
        print("CLOSED_HANDLERS", len(window.events.closed), flush=True)
        print("CALLS", ",".join(calls), flush=True)
        """
    )

    assert "CALLS start,stop" in result.stdout, result.stderr[-600:]
    # Closing the window must stop the API itself, not rely on webview.start() unwinding.
    assert "CLOSED_HANDLERS 1" in result.stdout, result.stderr[-600:]
