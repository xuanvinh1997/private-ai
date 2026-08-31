"""The frozen application's entry point — one binary, three roles.

A PyInstaller bundle contains exactly one executable, but Private AI is three programs:
the desktop shell, the ingestion worker, and the MCP servers an external client may want
to speak to over stdio. The console scripts in ``pyproject.toml`` cover that for a source
install; inside an ``.app`` there is no ``bin/`` and no PATH, so the role is chosen from
``argv`` instead and ``sys.executable`` — the bundle's own binary — is how one role starts
another.

The shell starts the worker itself. Reading a document is CPU-bound Python that holds the
GIL for as long as the file takes, which is precisely why ``private_ai.worker.loop`` is a
separate process; a bundled app that skipped it would freeze its own window on every
upload. Losing the worker is not fatal, though: the upload dialog processes inline when
nothing holds the claim, so a worker that fails to start costs responsiveness, not
function, and must never stop the app from opening.
"""

from __future__ import annotations

import atexit
import logging
import multiprocessing
import os
import subprocess
import sys

logger = logging.getLogger("private_ai.bundle")

WORKER_FLAG = "--worker"
MCP_FLAG = "--mcp"
ASR_FLAG = "--asr"

# MCP server module suffixes, as an external client would name them.
MCP_SERVERS = {
    "core": "core_server",
    "artifacts": "artifacts",
    "vector": "rag_vector",
    "keyword": "rag_keyword",
    "hybrid": "rag_hybrid",
    "graph": "rag_graph",
    "summary": "rag_summary",
    "web": "rag_web",
}

# Long enough for the worker to finish the chunk it is on and drop its claim, short
# enough that quitting the app never feels stuck.
WORKER_STOP_TIMEOUT_SECONDS = 5.0


def _run_worker() -> int:
    from private_ai.worker.loop import run

    run()
    return 0


def _run_asr(arguments: list[str]) -> int:
    """``--asr status`` reports what the speech stack found, ``--asr setup`` builds it.

    A packaged build ships the compiled runtime and downloads the weights from the models
    screen, so neither should be needed. ``status`` stays because it is the one question
    worth asking when someone reports that the microphone is grey, and it answers it
    without a debugger.
    """
    from private_ai.asr import manager

    sys.argv = [sys.argv[0], *arguments]
    manager.run()
    return 0


def _run_mcp(name: str) -> int:
    module = MCP_SERVERS.get(name)
    if module is None:
        sys.stderr.write(
            f"MCP server '{name}' không tồn tại. Chọn: {', '.join(sorted(MCP_SERVERS))}\n"
        )
        return 2
    import importlib

    importlib.import_module(f"private_ai.mcp.servers.{module}").run()
    return 0


def _start_worker() -> subprocess.Popen | None:
    """Launch this same binary again in its worker role, or carry on without one."""
    if os.environ.get("PRIVATE_AI_NO_WORKER"):
        return None
    try:
        return subprocess.Popen(  # noqa: S603 - argv is ours, not the user's
            [sys.executable, WORKER_FLAG],
            stdin=subprocess.DEVNULL,
            # Its own group, so a Ctrl-C aimed at the shell does not race us to the child.
            start_new_session=True,
        )
    except OSError:
        logger.exception("Không khởi động được tiến trình xử lý tài liệu")
        return None


def _stop_worker(worker: subprocess.Popen | None) -> None:
    if worker is None or worker.poll() is not None:
        return
    worker.terminate()
    try:
        worker.wait(timeout=WORKER_STOP_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        worker.kill()


def main() -> int:
    # Required before anything else on a frozen build: without it a child process would
    # re-enter this function and start a second copy of the whole application.
    multiprocessing.freeze_support()

    argv = sys.argv[1:]
    if argv and argv[0] == WORKER_FLAG:
        return _run_worker()
    if argv and argv[0] == MCP_FLAG:
        return _run_mcp(argv[1] if len(argv) > 1 else "core")
    if argv and argv[0] == ASR_FLAG:
        return _run_asr(argv[1:] or ["status"])

    worker = _start_worker()
    atexit.register(_stop_worker, worker)
    try:
        from private_ai.ui.app import main as run_ui

        return run_ui()
    finally:
        _stop_worker(worker)


if __name__ == "__main__":
    sys.exit(main())
