"""Application settings.

Every field is an environment variable prefixed ``PRIVATE_AI_``. The desktop app,
the ingestion worker and each MCP server all read the same object, so a value set
once applies everywhere.
"""

from __future__ import annotations

import os
import platform
import subprocess
import sys
from functools import lru_cache
from pathlib import Path

from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

FALLBACK_GPU_CAPACITY_BYTES = 96 * 1024**3

# Where a packaged build keeps its state, under the user's home directory. One name on
# every platform rather than the three each platform would prefer: this is a single-user
# local application whose data a person genuinely does open a terminal to inspect, back up
# with a script, or delete outright, and one path that is the same everywhere and needs no
# shell escaping is worth more here than matching each OS's filing convention.
BUNDLED_DATA_FOLDER = ".private-ai"


def default_data_dir() -> Path:
    """Where state lives when nothing sets ``PRIVATE_AI_DATA_DIR``.

    A source checkout keeps it beside the source as ``.local-data``, which is what makes a
    clone self-contained and a reset a single ``rm -rf``. A packaged app cannot: it is
    launched with the working directory set to ``/``, and its own directory is inside a
    read-only, code-signed bundle. So a frozen build resolves under the home directory
    instead, and the two never collide.
    """
    if not getattr(sys, "frozen", False):
        return Path(".local-data")
    return Path.home() / BUNDLED_DATA_FOLDER


def _sysctl_int(name: str) -> int | None:
    try:
        result = subprocess.run(  # noqa: S603
            ["/usr/sbin/sysctl", "-n", name],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0:
        return None
    try:
        return int(result.stdout.strip())
    except ValueError:
        return None


def is_unified_memory() -> bool:
    """True on Apple Silicon, where the GPU shares one pool with the CPU."""
    return platform.system() == "Darwin" and platform.machine() == "arm64"


def total_memory_bytes() -> int | None:
    """Physical RAM, when the platform lets us read it cheaply."""
    if platform.system() == "Darwin":
        return _sysctl_int("hw.memsize")
    return None


def detect_gpu_capacity_bytes() -> int:
    """Budget the GPU may actually use, rather than assuming the target machine.

    On Apple Silicon there is no separate VRAM: the GPU draws from system RAM, capped by
    ``iogpu.wired_limit_mb`` when set, and otherwise by the share macOS reserves by default
    (about three quarters on large-memory machines, two thirds on smaller ones).
    """
    if not is_unified_memory():
        return FALLBACK_GPU_CAPACITY_BYTES
    total = total_memory_bytes()
    if not total:
        return FALLBACK_GPU_CAPACITY_BYTES
    wired_limit_mb = _sysctl_int("iogpu.wired_limit_mb") or 0
    if wired_limit_mb > 0:
        return wired_limit_mb * 1024**2
    share = 0.75 if total >= 36 * 1024**3 else 2 / 3
    return int(total * share)


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_file=".env",
        env_prefix="PRIVATE_AI_",
        extra="ignore",
    )

    app_name: str = "Private AI"
    data_dir: Path = Field(default_factory=default_data_dir)

    # --- providers -------------------------------------------------------
    ollama_url: str = "http://127.0.0.1:11434"
    embedding_enabled: bool = True
    embedding_model: str = "embeddinggemma"
    vision_model: str = ""
    request_timeout_seconds: float = Field(default=60.0, gt=0)
    # Generation is not a request like the others. Listing models should fail fast, but a
    # local 27B answering from a long document legitimately takes minutes, and a summary
    # map-reduce spends a full model call on every batch. Sixty seconds killed those turns
    # mid-generation, so token production gets its own, far longer budget.
    generation_timeout_seconds: float = Field(default=600.0, gt=0)

    # --- agent -----------------------------------------------------------
    # Recursion budget for the LangGraph agent. Each tool call plus its follow-up
    # model turn costs two steps, so this is roughly four tool rounds plus an answer.
    agent_max_iterations: int = Field(default=10, ge=1, le=64)
    agent_stream_tokens: bool = True
    skills_enabled: bool = True
    # Extra directories scanned for SKILL.md packs, on top of the built-ins and
    # ``data_dir/skills``. Joined with the platform path separator.
    skill_paths: str = ""

    # --- retrieval -------------------------------------------------------
    retrieval_default_strategy: str = "auto"
    retrieval_top_k: int = Field(default=5, ge=1, le=50)
    # Hard ceiling on retrieved text placed in the system prompt, in characters. A budget
    # is what stops an exhaustive strategy from filling the window: roughly four
    # characters per token, so the default is about 6k tokens of context.
    retrieval_context_chars: int = Field(default=24000, ge=1000, le=400000)
    retrieval_chunk_size: int = Field(default=1400, ge=200, le=8000)
    retrieval_chunk_overlap: int = Field(default=180, ge=0, le=2000)
    embedding_batch_size: int = Field(default=32, ge=1, le=256)
    embedding_concurrency: int = Field(default=4, ge=1, le=32)

    # --- MCP -------------------------------------------------------------
    mcp_host: str = "127.0.0.1"
    mcp_port: int = Field(default=8010, ge=1, le=65535)
    mcp_require_auth: bool = True
    # External MCP servers the agent should also connect to, as a JSON object of
    # ``{"name": {"command": ..., "args": [...]}}`` or ``{"name": {"url": ...}}``.
    mcp_external_servers: str = ""

    # --- ASR -------------------------------------------------------------
    asr_enabled: bool = True
    asr_executable: Path | None = None
    asr_model: Path | None = None
    asr_language: str = "vi-VN"
    ffmpeg_executable: str = ""

    # --- GPU -------------------------------------------------------------
    gpu_capacity_bytes: int = Field(default_factory=detect_gpu_capacity_bytes, gt=0)
    gpu_model_overhead_ratio: float = Field(default=1.1, ge=1.0, le=3.0)
    asr_vram_reservation_bytes: int = Field(default=2 * 1024**3, ge=0)

    # --- files and uploads ----------------------------------------------
    web_search_timeout_seconds: float = Field(default=20.0, gt=0)
    # Folders the file tools may read without asking. Empty means every path needs the
    # user's approval at the moment it is first requested.
    file_roots: str = ""
    file_read_max_bytes: int = Field(default=1024 * 1024, gt=0)
    max_upload_bytes: int = Field(default=100 * 1024 * 1024, gt=0)

    # --- ingestion -------------------------------------------------------
    # Parsing, chunking and graph merging are pure Python and hold the GIL, so running
    # them inside the UI process stalls the event loop for as long as a file takes. The
    # desktop app leaves this off and spawns ``private-ai-worker``; a headless script with
    # no second process can turn it on.
    inline_ingestion: bool = False
    worker_poll_seconds: float = Field(default=2.0, gt=0)

    # --- UI --------------------------------------------------------------
    ui_theme: str = "light"
    ui_font_scale: str = "normal"
    ui_language: str = "vi"

    @field_validator("data_dir", mode="after")
    @classmethod
    def resolve_data_dir(cls, value: Path) -> Path:
        return value.expanduser().resolve()

    @property
    def file_root_paths(self) -> list[Path]:
        """Split on the platform's path separator so Windows drive letters survive."""
        return [
            Path(item).expanduser().resolve()
            for item in self.file_roots.split(os.pathsep)
            if item.strip()
        ]

    @property
    def skill_path_list(self) -> list[Path]:
        return [
            Path(item).expanduser().resolve()
            for item in self.skill_paths.split(os.pathsep)
            if item.strip()
        ]

    @property
    def database_path(self) -> Path:
        return self.data_dir / "private-ai.db"

    @property
    def documents_dir(self) -> Path:
        return self.data_dir / "documents"

    @property
    def mcp_token_path(self) -> Path:
        return self.data_dir / "mcp-token"

    @property
    def lightrag_dir(self) -> Path:
        return self.data_dir / "lightrag"

    @property
    def skills_dir(self) -> Path:
        return self.data_dir / "skills"

    @property
    def artifacts_dir(self) -> Path:
        """Files the agent produced. Deliberately not under ``documents``: what the user
        ingested and what a model generated must never share a folder."""
        return self.data_dir / "artifacts"

    @property
    def asr_dir(self) -> Path:
        return self.data_dir / "asr"

    @property
    def default_asr_model_path(self) -> Path:
        return self.asr_dir / "models" / "nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf"

    @property
    def platform_name(self) -> str:
        return platform.system().lower()


@lru_cache
def get_settings() -> Settings:
    return Settings()
