from __future__ import annotations

import os
import signal
import socket
import subprocess
import sys
import time
from contextlib import suppress
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


def windows_kill_on_close_job() -> int | None:
    """A job whose handle, once dropped, takes every process inside it down.

    Windows has no parent-death signal, so a crashed launcher would otherwise leave the API
    running with no window attached to it. Closing this handle -- including when the process
    dies without unwinding -- terminates the whole job.
    """
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    class BasicLimits(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.POINTER(ctypes.c_ulong)),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class IoCounters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint64) for name in (
            "ReadOperationCount",
            "WriteOperationCount",
            "OtherOperationCount",
            "ReadTransferCount",
            "WriteTransferCount",
            "OtherTransferCount",
        )]

    class ExtendedLimits(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", BasicLimits),
            ("IoInfo", IoCounters),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    job = kernel32.CreateJobObjectW(None, None)
    if not job:
        return None
    limits = ExtendedLimits()
    limits.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE
    if not kernel32.SetInformationJobObject(
        job,
        9,  # JobObjectExtendedLimitInformation
        ctypes.byref(limits),
        ctypes.sizeof(limits),
    ):
        kernel32.CloseHandle(job)
        return None
    return job


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
    worker: subprocess.Popen[bytes] | None = field(default=None, init=False)
    log_path: Path | None = field(default=None, init=False)
    worker_log_path: Path | None = field(default=None, init=False)
    job: int | None = field(default=None, init=False)

    @property
    def api_url(self) -> str:
        return f"http://{self.host}:{self.port}"

    def _environment(self) -> tuple[Path, dict[str, str]]:
        environment = os.environ.copy()
        root = workspace_root()
        api_source = root / "services" / "api" / "src"
        existing_pythonpath = environment.get("PYTHONPATH")
        environment["PYTHONPATH"] = os.pathsep.join(
            part for part in (str(api_source), existing_pythonpath) if part
        )
        return root, environment

    def command(self) -> tuple[list[str], Path | None, dict[str, str]]:
        root, environment = self._environment()
        # Document parsing holds the GIL for as long as a file takes, so the API must not
        # be the process doing it: worker_command() below runs that work instead.
        environment["PRIVATE_AI_INLINE_INGESTION"] = "0"
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

    def worker_command(self) -> tuple[list[str], Path | None, dict[str, str]]:
        root, environment = self._environment()
        return ([sys.executable, "-m", "private_ai_api.worker"], root, environment)

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
        self.worker_log_path = data_dir(cwd or Path.cwd()) / "desktop-worker.log"
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        # The window has no console on Windows, so the API needs somewhere to explain itself.
        self.process = self._spawn(command, cwd, environment, self.log_path)
        self._contain(self.process)
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise self.failure(
                    f"The Private AI API exited with code {self.process.returncode} "
                    f"before it accepted connections."
                )
            if self.is_ready():
                self._start_worker()
                return
            time.sleep(0.2)
        self.stop()
        raise self.failure(
            f"The Private AI API did not answer {self.api_url}/api/v1/health/live "
            f"within {timeout_seconds:.0f} seconds."
        )

    def _spawn(
        self,
        command: list[str],
        cwd: Path | None,
        environment: dict[str, str],
        log: Path,
    ) -> subprocess.Popen[bytes]:
        with log.open("wb") as handle:
            return subprocess.Popen(  # noqa: S603
                command,
                cwd=cwd,
                env=environment,
                stdout=handle,
                stderr=handle,
                **self._containment(),
            )

    def _start_worker(self) -> None:
        """Start ingestion only once the API is up, because the API owns the migrations.

        A worker that cannot start is not fatal: the queue simply waits, and the user sees
        documents sitting at "queued" rather than an app that refuses to open.
        """
        command, cwd, environment = self.worker_command()
        log = self.worker_log_path or data_dir(cwd or Path.cwd()) / "desktop-worker.log"
        try:
            self.worker = self._spawn(command, cwd, environment, log)
        except OSError:
            self.worker = None
            return
        self._contain(self.worker)

    def is_ready(self) -> bool:
        """Probe liveness, not /health: that endpoint waits on Ollama and the active provider."""
        request = Request(f"{self.api_url}/api/v1/health/live")  # noqa: S310
        try:
            with build_opener(ProxyHandler({})).open(request, timeout=PROBE_TIMEOUT) as response:
                return response.status == 200
        except (URLError, TimeoutError, OSError):
            return False

    @staticmethod
    def _containment() -> dict[str, object]:
        """Spawn the API as the head of its own group, so its children can be killed too.

        The API starts helpers of its own -- FFmpeg and the transcription binary -- and
        terminating uvicorn alone would leave those running with nothing attached to them.
        """
        if os.name == "nt":
            return {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
        return {"start_new_session": True}

    def _contain(self, process: subprocess.Popen[bytes]) -> None:
        if os.name != "nt":
            return
        # One job holds every process we start, so closing its handle takes them all down.
        if self.job is None:
            self.job = windows_kill_on_close_job()
        if not self.job:
            return
        import ctypes

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.AssignProcessToJobObject(self.job, int(process._handle))  # noqa: SLF001

    def _signal_tree(self, number: int, process: subprocess.Popen[bytes] | None = None) -> None:
        """Signal the whole group, falling back to the single process when that fails."""
        process = process if process is not None else self.process
        if process is None:
            return
        if os.name == "nt":
            # The job object owns the tree here; the group signal only reaches the console app.
            process.kill() if number == signal.SIGKILL else process.terminate()
            return
        try:
            os.killpg(os.getpgid(process.pid), number)
        except (ProcessLookupError, PermissionError, OSError):
            process.kill() if number == signal.SIGKILL else process.terminate()

    def _release_job(self) -> None:
        if not self.job:
            return
        import ctypes

        # Dropping the last handle is what kills anything still inside the job.
        ctypes.WinDLL("kernel32", use_last_error=True).CloseHandle(self.job)
        self.job = None

    def _terminate(self, process: subprocess.Popen[bytes] | None, grace: float) -> None:
        if process is None or process.poll() is not None:
            return
        self._signal_tree(signal.SIGTERM, process)
        try:
            process.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            self._signal_tree(signal.SIGKILL, process)
            with suppress(subprocess.TimeoutExpired):
                process.wait(timeout=3)

    def stop(self) -> None:
        """Take the API, the worker and everything they started down. Safe to call twice."""
        # The worker goes first: it releases its document claim on the way out, so the next
        # launch sees a free queue instead of one that has to time the claim out.
        self._terminate(self.worker, grace=8)
        self.worker = None
        self._terminate(self.process, grace=8)
        self._release_job()
        self.process = None
