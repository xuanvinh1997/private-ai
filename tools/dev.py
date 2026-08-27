from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def executable(name: str) -> str:
    found = shutil.which(name)
    if found:
        return found
    if os.name == "nt":
        found = shutil.which(f"{name}.cmd")
        if found:
            return found
    raise RuntimeError(f"Required executable is not installed: {name}")


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

    processes: list[subprocess.Popen[bytes]] = []
    try:
        if not args.web_only:
            command, environment = api_command()
            processes.append(subprocess.Popen(command, cwd=ROOT, env=environment))
            if not args.no_mcp:
                command, mcp_environment = mcp_command(environment)
                processes.append(
                    subprocess.Popen(command, cwd=ROOT, env=mcp_environment)
                )
        if not args.api_only:
            processes.append(subprocess.Popen(web_command(), cwd=ROOT / "apps" / "web"))
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
            if process.poll() is None:
                process.terminate()
        for process in processes:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()


if __name__ == "__main__":
    main()
