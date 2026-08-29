from __future__ import annotations

import os
import platform
import subprocess
from functools import lru_cache
from pathlib import Path

from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

FALLBACK_GPU_CAPACITY_BYTES = 96 * 1024**3


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
    host: str = "127.0.0.1"
    port: int = Field(default=8000, ge=1, le=65535)
    data_dir: Path = Path(".local-data")
    frontend_dist: Path = Path("apps/web/dist")
    ollama_url: str = "http://127.0.0.1:11434"
    embedding_enabled: bool = True
    embedding_model: str = "embeddinggemma"
    vision_model: str = ""
    mcp_host: str = "127.0.0.1"
    mcp_port: int = Field(default=8010, ge=1, le=65535)
    mcp_require_auth: bool = True
    asr_enabled: bool = True
    asr_executable: Path | None = None
    asr_model: Path | None = None
    asr_language: str = "vi-VN"
    ffmpeg_executable: str = ""
    gpu_capacity_bytes: int = Field(default_factory=detect_gpu_capacity_bytes, gt=0)
    gpu_model_overhead_ratio: float = Field(default=1.1, ge=1.0, le=3.0)
    asr_vram_reservation_bytes: int = Field(default=2 * 1024**3, ge=0)
    request_timeout_seconds: float = Field(default=60.0, gt=0)
    web_search_timeout_seconds: float = Field(default=20.0, gt=0)
    # Folders the file tools may read without asking. Empty means every path needs the
    # user's approval at the moment it is first requested.
    file_roots: str = ""
    file_read_max_bytes: int = Field(default=1024 * 1024, gt=0)
    max_upload_bytes: int = Field(default=100 * 1024 * 1024, gt=0)
    # Ingestion is CPU-bound Python: parsing, chunking and graph merging all hold the GIL,
    # so running it in the API process stalls every request for as long as a file takes.
    # The desktop launcher turns this off and starts private-ai-worker instead; it stays on
    # for a bare `uvicorn private_ai_api.main:app`, where there is no second process.
    inline_ingestion: bool = True
    worker_poll_seconds: float = Field(default=2.0, gt=0)

    @field_validator("data_dir", mode="after")
    @classmethod
    def resolve_data_dir(cls, value: Path) -> Path:
        return value.expanduser().resolve()

    @field_validator("frontend_dist", mode="after")
    @classmethod
    def resolve_frontend_dist(cls, value: Path) -> Path:
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
