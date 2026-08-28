from __future__ import annotations

import argparse
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from urllib.error import URLError
from urllib.request import ProxyHandler, build_opener

ROOT = Path(__file__).resolve().parent.parent
API_HOST = "127.0.0.1"
API_PORT = 8000


def executable(name: str) -> str:
    found = shutil.which(name)
    if found:
        return found
    if os.name == "nt":
        found = shutil.which(f"{name}.cmd")
        if found:
            return found
    raise RuntimeError(f"Required executable is not installed: {name}")


def spawn(
    command: list[str],
    cwd: Path,
    environment: dict[str, str] | None = None,
) -> subprocess.Popen[bytes]:
    """Give each service its own process group so reload workers cannot outlive us."""
    if os.name == "nt":
        return subprocess.Popen(
            command, cwd=cwd, env=environment, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP
        )
    return subprocess.Popen(command, cwd=cwd, env=environment, start_new_session=True)


def signal_group(process: subprocess.Popen[bytes], number: int) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        process.kill() if number == signal.SIGKILL else process.terminate()
        return
    try:
        os.killpg(os.getpgid(process.pid), number)
    except (ProcessLookupError, PermissionError):
        process.kill() if number == signal.SIGKILL else process.terminate()


def port_holder(host: str, port: int) -> str | None:
    """Name whatever already listens, so a clash is not just 'Address already in use'."""
    try:
        with socket.create_connection((host, port), timeout=1.0):
            pass
    except OSError:
        return None
    # A system proxy must never be consulted for a loopback address.
    opener = build_opener(ProxyHandler({}))
    try:
        with opener.open(f"http://{host}:{port}/api/v1/health/live", timeout=2.0) as response:
            if response.status == 200:
                return "a Private AI API is already running there"
    except (URLError, OSError):
        pass
    return "another application is holding it"


def api_command() -> tuple[list[str], dict[str, str]]:
    environment = os.environ.copy()
    source = ROOT / "services" / "api" / "src"
    environment["PYTHONPATH"] = os.pathsep.join(
        part for part in (str(source), environment.get("PYTHONPATH")) if part
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
            "--reload",
        ],
        environment,
    )


def mcp_command(environment: dict[str, str]) -> tuple[list[str], dict[str, str]]:
    return ([sys.executable, "-m", "private_ai_api.mcp_server"], environment.copy())


def web_command() -> list[str]:
    pnpm = shutil.which("pnpm") or (shutil.which("pnpm.cmd") if os.name == "nt" else None)
    if pnpm:
        return [pnpm, "dev"]
    corepack = shutil.which("corepack") or (
        shutil.which("corepack.cmd") if os.name == "nt" else None
    )
    if corepack:
        return [corepack, "pnpm", "dev"]
    npx = shutil.which("npx") or (shutil.which("npx.cmd") if os.name == "nt" else None)
    if npx:
        return [npx, "--yes", "pnpm@10.17.1", "dev"]
    raise RuntimeError("Install pnpm, Corepack, or npm/npx to run the web application")


def main() -> None:
    parser = argparse.ArgumentParser(description="Run Private AI development services")
    parser.add_argument("--api-only", action="store_true")
    parser.add_argument("--web-only", action="store_true")
    parser.add_argument("--no-mcp", action="store_true")
    args = parser.parse_args()
    if args.api_only and args.web_only:
        parser.error("--api-only and --web-only cannot be combined")

    def interrupt(number: int, frame: object) -> None:
        raise KeyboardInterrupt

    # Register both explicitly: a child of a non-interactive shell inherits SIGINT as SIG_IGN,
    # and detached process groups no longer receive the terminal's signals on our behalf.
    signal.signal(signal.SIGINT, interrupt)
    signal.signal(signal.SIGTERM, interrupt)

    processes: list[subprocess.Popen[bytes]] = []
    try:
        if not args.web_only:
            if holder := port_holder(API_HOST, API_PORT):
                lookup = (
                    f"netstat -ano | findstr :{API_PORT}"
                    if os.name == "nt"
                    else f"lsof -nP -iTCP:{API_PORT} -sTCP:LISTEN"
                )
                parser.exit(
                    1,
                    f"Port {API_PORT} is busy: {holder}. Stop it and retry, run "
                    f"'{sys.executable} tools/dev.py --web-only', or find the process with "
                    f"'{lookup}'.\n",
                )
            command, environment = api_command()
            processes.append(spawn(command, ROOT, environment))
            if not args.no_mcp:
                command, mcp_environment = mcp_command(environment)
                processes.append(spawn(command, ROOT, mcp_environment))
        if not args.api_only:
            processes.append(spawn(web_command(), ROOT / "apps" / "web"))
        while processes:
            if exited := next(
                (process for process in processes if process.poll() is not None),
                None,
            ):
                raise SystemExit(exited.returncode)
            time.sleep(0.25)
    except KeyboardInterrupt:
        pass
    finally:
        for process in processes:
            signal_group(process, signal.SIGTERM)
        for process in processes:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                signal_group(process, signal.SIGKILL)


if __name__ == "__main__":
    main()
