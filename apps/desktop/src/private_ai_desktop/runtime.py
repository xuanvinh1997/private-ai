from __future__ import annotations

import os
import platform
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen


def workspace_root() -> Path:
    configured = os.getenv("PRIVATE_AI_PROJECT_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    for parent in Path(__file__).resolve().parents:
        if (parent / "GOAL.md").exists() and (parent / "services" / "api").exists():
            return parent
    raise RuntimeError("Cannot locate the Private AI workspace")


@dataclass(slots=True)
class RuntimeController:
    api_url: str = "http://127.0.0.1:8000"
    mode: str = "auto"
    process: subprocess.Popen[bytes] | None = field(default=None, init=False)

    def resolved_mode(self) -> str:
        requested = os.getenv("PRIVATE_AI_DESKTOP_RUNTIME", self.mode).lower()
        if requested not in {"auto", "local", "wsl"}:
            raise ValueError("PRIVATE_AI_DESKTOP_RUNTIME must be auto, local, or wsl")
        if requested == "auto":
            return "wsl" if platform.system() == "Windows" else "local"
        if requested == "wsl" and platform.system() != "Windows":
            raise RuntimeError("WSL runtime can only be selected on Windows")
        return requested

    def command(self) -> tuple[list[str], Path | None, dict[str, str]]:
        environment = os.environ.copy()
        if self.resolved_mode() == "wsl":
            distro = os.getenv("PRIVATE_AI_WSL_DISTRO", "Ubuntu")
            project_dir = os.getenv("PRIVATE_AI_WSL_PROJECT_DIR", "/opt/private-ai")
            executable = os.getenv(
                "PRIVATE_AI_WSL_API_EXECUTABLE",
                f"{project_dir}/.venv/bin/private-ai-api",
            )
            return (
                ["wsl.exe", "--distribution", distro, "--cd", project_dir, "--", executable],
                None,
                environment,
            )

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
                "127.0.0.1",
                "--port",
                "8000",
            ],
            root,
            environment,
        )

    def start(self, timeout_seconds: float = 30.0) -> None:
        if self.is_ready():
            return
        command, cwd, environment = self.command()
        self.process = subprocess.Popen(command, cwd=cwd, env=environment)
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"API process exited with code {self.process.returncode}")
            if self.is_ready():
                return
            time.sleep(0.2)
        self.stop()
        raise TimeoutError("Private AI API did not become ready in time")

    def is_ready(self) -> bool:
        try:
            with urlopen(f"{self.api_url}/api/v1/health", timeout=0.5) as response:  # noqa: S310
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

