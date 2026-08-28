from __future__ import annotations

import os
import socket
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

LOG_TAIL_BYTES = 4000
PROBE_TIMEOUT = 5.0


def workspace_root() -> Path:
    configured = os.getenv("PRIVATE_AI_PROJECT_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    for parent in Path(__file__).resolve().parents:
        if (parent / "GOAL.md").exists() and (parent / "services" / "api").exists():
            return parent
    raise RuntimeError(
        "Cannot locate the Private AI workspace. Install the packages with "
        "'pip install --editable' from a checkout, or point PRIVATE_AI_PROJECT_DIR at one."
    )


def data_dir(root: Path) -> Path:
    configured = os.getenv("PRIVATE_AI_DATA_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return root / ".local-data"


def frontend_dist(root: Path) -> Path:
    configured = os.getenv("PRIVATE_AI_FRONTEND_DIST")
    if configured:
        return Path(configured).expanduser().resolve()
    return root / "apps" / "web" / "dist"


def port_is_taken(host: str, port: int) -> bool:
    # connect_ex on a socket with a timeout reports EINPROGRESS, so connect and catch instead.
    try:
        with socket.create_connection((host, port), timeout=1.0):
            return True
    except OSError:
        return False


@dataclass(slots=True)
class RuntimeController:
    host: str = field(default_factory=lambda: os.getenv("PRIVATE_AI_HOST", "127.0.0.1"))
    port: int = field(default_factory=lambda: int(os.getenv("PRIVATE_AI_PORT", "8000")))
    process: subprocess.Popen[bytes] | None = field(default=None, init=False)
    log_path: Path | None = field(default=None, init=False)

    @property
    def api_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def command(self) -> tuple[list[str], Path | None, dict[str, str]]:
        environment = os.environ.copy()
        root = workspace_root()
        api_source = root / "services" / "api" / "src"
        existing_pythonpath = environment.get("PYTHONPATH")
        environment["PYTHONPATH"] = os.pathsep.join(
            part for part in (str(api_source), existing_pythonpath) if part
        )
        return (
            [
                sys.executable,
                "-m",
                "uvicorn",
                "private_ai_api.main:app",
                "--host",
                self.host,
                "--port",
                str(self.port),
            ],
            root,
            environment,
        )

    def preflight(self) -> None:
        """Fail with an actionable message instead of an empty window."""
        root = workspace_root()
        if not (frontend_dist(root) / "index.html").is_file():
            raise RuntimeError(
                f"The web interface has not been built, so {self.api_url} would only answer with "
                f"API routes. Run 'pnpm --dir apps/web build' first."
            )

    def log_tail(self) -> str:
        if not self.log_path or not self.log_path.exists():
            return ""
        with self.log_path.open("rb") as handle:
            handle.seek(0, os.SEEK_END)
            handle.seek(max(0, handle.tell() - LOG_TAIL_BYTES))
            return handle.read().decode("utf-8", "replace").strip()

    def failure(self, summary: str) -> RuntimeError:
        tail = self.log_tail()
        if tail:
            return RuntimeError(f"{summary}\n\nAPI log ({self.log_path}):\n{tail}")
        return RuntimeError(summary)

    def start(self, timeout_seconds: float = 90.0) -> None:
        if self.is_ready():
            return
        self.preflight()
        if port_is_taken(self.host, self.port):
            raise RuntimeError(
                f"Port {self.port} is held by another application that is not the Private AI API. "
                f"Close it, or set PRIVATE_AI_PORT to a free port."
            )
        command, cwd, environment = self.command()
        self.log_path = data_dir(cwd or Path.cwd()) / "desktop-api.log"
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        # The window has no console on Windows, so the API needs somewhere to explain itself.
        with self.log_path.open("wb") as log:
            self.process = subprocess.Popen(
                command, cwd=cwd, env=environment, stdout=log, stderr=log
            )
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise self.failure(
                    f"The Private AI API exited with code {self.process.returncode} "
                    f"before it accepted connections."
                )
            if self.is_ready():
                return
            time.sleep(0.2)
        self.stop()
        raise self.failure(
            f"The Private AI API did not answer {self.api_url}/api/v1/health/live "
            f"within {timeout_seconds:.0f} seconds."
        )

    def is_ready(self) -> bool:
        """Probe liveness, not /health: that endpoint waits on Ollama and the active provider."""
        request = Request(f"{self.api_url}/api/v1/health/live")  # noqa: S310
        try:
            with build_opener(ProxyHandler({})).open(request, timeout=PROBE_TIMEOUT) as response:
                return response.status == 200
        except (URLError, TimeoutError, OSError):
            return False

    def stop(self) -> None:
        if not self.process or self.process.poll() is not None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
