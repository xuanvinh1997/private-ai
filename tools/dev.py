"""Run Private AI in development.

There is no API server, no bundler and no dev server any more: the desktop process owns
the event loop and calls the service layer directly. What is left to supervise is two
processes — the Qt app and the ingestion worker — and the reason they are separate is
the GIL, not a network boundary.

The process-group handling below is the part worth keeping from the old launcher. Each
child is started in its own group (a Job-friendly ``CREATE_NEW_PROCESS_GROUP`` on
Windows) so that stopping this script kills the whole tree, including anything the app
spawned itself, rather than leaving orphans holding an ingestion claim.
"""

from __future__ import annotations

import argparse
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "src"

# Each RAG strategy is its own MCP server, and every one of them is runnable on stdio.
# ``core`` is the non-retrieval half: workspaces, documents, memory, models, files.
MCP_SERVERS: dict[str, str] = {
    "core": "private_ai.mcp.servers.core_server",
    "vector": "private_ai.mcp.servers.rag_vector",
    "keyword": "private_ai.mcp.servers.rag_keyword",
    "hybrid": "private_ai.mcp.servers.rag_hybrid",
    "graph": "private_ai.mcp.servers.rag_graph",
    "summary": "private_ai.mcp.servers.rag_summary",
    "web": "private_ai.mcp.servers.rag_web",
}

SHUTDOWN_GRACE_SECONDS = 8
POLL_SECONDS = 0.25


def environment(**overrides: str) -> dict[str, str]:
    """The child's environment, with ``src`` on the path for a non-installed checkout."""
    values = os.environ.copy()
    values["PYTHONPATH"] = os.pathsep.join(
        part for part in (str(SOURCE), values.get("PYTHONPATH")) if part
    )
    values.update(overrides)
    return values


def spawn(command: list[str], env: dict[str, str]) -> subprocess.Popen[bytes]:
    """Give each child its own process group so nothing it starts can outlive us."""
    if os.name == "nt":
        return subprocess.Popen(  # noqa: S603
            command,
            cwd=ROOT,
            env=env,
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
        )
    return subprocess.Popen(command, cwd=ROOT, env=env, start_new_session=True)  # noqa: S603


def signal_group(process: subprocess.Popen[bytes], number: int) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        # Windows has no process-group signalling worth the name; terminate the root and
        # let the group go with it.
        process.kill() if number == signal.SIGKILL else process.terminate()
        return
    try:
        os.killpg(os.getpgid(process.pid), number)
    except (ProcessLookupError, PermissionError):
        process.kill() if number == signal.SIGKILL else process.terminate()


def shut_down(processes: list[subprocess.Popen[bytes]]) -> None:
    """Stop the worker first so it releases its ingestion claims before the app goes."""
    for process in reversed(processes):
        signal_group(process, signal.SIGTERM)
    for process in reversed(processes):
        try:
            process.wait(timeout=SHUTDOWN_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            signal_group(process, signal.SIGKILL)


def serve_one_mcp_server(strategy: str) -> int:
    """Speak MCP on this process's own stdin/stdout, for an inspector or a client config.

    Run in process rather than spawned: stdio *is* the transport, so a supervising parent
    would only have to proxy the pipes it already owns.
    """
    from importlib import import_module

    if str(SOURCE) not in sys.path:
        sys.path.insert(0, str(SOURCE))
    module = import_module(MCP_SERVERS[strategy])
    module.run()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Chạy Private AI ở chế độ phát triển")
    parser.add_argument(
        "--no-worker",
        action="store_true",
        help="Không sinh tiến trình đọc tài liệu; ứng dụng tự đọc trong tiến trình của nó",
    )
    parser.add_argument(
        "--mcp",
        choices=sorted(MCP_SERVERS),
        help="Chạy riêng một MCP server trên stdio để gỡ lỗi, thay vì mở ứng dụng",
    )
    parser.add_argument(
        "--worker-only",
        action="store_true",
        help="Chỉ chạy tiến trình đọc tài liệu, để bám vào một cơ sở dữ liệu đang có",
    )
    arguments = parser.parse_args()

    if arguments.mcp:
        if arguments.no_worker or arguments.worker_only:
            parser.error("--mcp không đi cùng --no-worker hoặc --worker-only")
        return serve_one_mcp_server(arguments.mcp)
    if arguments.worker_only and arguments.no_worker:
        parser.error("--worker-only và --no-worker loại trừ nhau")

    def interrupt(number: int, frame: object) -> None:
        raise KeyboardInterrupt

    # Registered explicitly: a child of a non-interactive shell inherits SIGINT as
    # SIG_IGN, and a detached process group no longer receives the terminal's signals on
    # our behalf.
    signal.signal(signal.SIGINT, interrupt)
    signal.signal(signal.SIGTERM, interrupt)

    processes: list[subprocess.Popen[bytes]] = []
    try:
        run_worker = not arguments.no_worker
        if run_worker:
            # The worker waits for the app's migrations, so the start order does not matter.
            processes.append(spawn([sys.executable, "-m", "private_ai.worker"], environment()))
        if not arguments.worker_only:
            processes.append(
                spawn(
                    [sys.executable, "-m", "private_ai"],
                    environment(PRIVATE_AI_INLINE_INGESTION="0" if run_worker else "1"),
                )
            )

        while processes:
            exited = next(
                (process for process in processes if process.poll() is not None),
                None,
            )
            if exited is not None:
                # Either child going down takes the session with it: a worker that
                # crashed leaves uploads stuck, and a closed window means we are done.
                return exited.returncode or 0
            time.sleep(POLL_SECONDS)
    except KeyboardInterrupt:
        pass
    finally:
        shut_down(processes)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
