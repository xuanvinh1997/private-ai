from __future__ import annotations

import argparse
import os
import shutil
import subprocess
from pathlib import Path

from private_ai_api.config import Settings

ROOT = Path(__file__).resolve().parents[4]


def docker_command() -> str:
    docker = shutil.which("docker") or (
        shutil.which("docker.exe") if os.name == "nt" else None
    )
    if not docker:
        raise RuntimeError("Docker Desktop/Engine is not installed or not in PATH")
    return docker


def run() -> None:
    parser = argparse.ArgumentParser(description="Manage the loopback-only Neo4j service")
    parser.add_argument(
        "action",
        choices=("up", "down", "status", "logs"),
        nargs="?",
        default="up",
    )
    args = parser.parse_args()
    settings = Settings()
    environment = os.environ.copy()
    environment["PRIVATE_AI_NEO4J_PASSWORD"] = settings.resolved_neo4j_password(create=True)
    command = [
        docker_command(),
        "compose",
        "--file",
        str(ROOT / "infra" / "compose.yaml"),
    ]
    if args.action == "up":
        command.extend(["up", "--detach", "--wait"])
    elif args.action == "down":
        command.append("down")
    elif args.action == "status":
        command.append("ps")
    else:
        command.extend(["logs", "--tail", "100", "neo4j"])
    subprocess.run(command, cwd=ROOT, env=environment, check=True)
    if args.action == "up":
        print(f"Neo4j is ready at {settings.neo4j_url}")
        print(f"Password is stored locally at {settings.neo4j_password_path}")


if __name__ == "__main__":
    run()
