from __future__ import annotations

import platform
import secrets
from contextlib import suppress
from functools import lru_cache
from pathlib import Path
from typing import Literal

from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


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
    neo4j_url: str = "bolt://127.0.0.1:7687"
    neo4j_user: str = "neo4j"
    neo4j_password: str = ""
    neo4j_database: str = "neo4j"
    neo4j_enabled: bool = True
    graph_entity_model: str = ""
    asr_enabled: bool = True
    asr_executable: Path | None = None
    asr_model: Path | None = None
    asr_language: str = "vi-VN"
    ffmpeg_executable: str = ""
    gpu_capacity_bytes: int = Field(default=96 * 1024**3, gt=0)
    gpu_model_overhead_ratio: float = Field(default=1.1, ge=1.0, le=3.0)
    asr_vram_reservation_bytes: int = Field(default=2 * 1024**3, ge=0)
    desktop_runtime: Literal["auto", "local", "wsl"] = "auto"
    request_timeout_seconds: float = Field(default=60.0, gt=0)
    max_upload_bytes: int = Field(default=100 * 1024 * 1024, gt=0)

    @field_validator("data_dir", mode="after")
    @classmethod
    def resolve_data_dir(cls, value: Path) -> Path:
        return value.expanduser().resolve()

    @field_validator("frontend_dist", mode="after")
    @classmethod
    def resolve_frontend_dist(cls, value: Path) -> Path:
        return value.expanduser().resolve()

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
    def neo4j_password_path(self) -> Path:
        return self.data_dir / "neo4j-password"

    @property
    def asr_dir(self) -> Path:
        return self.data_dir / "asr"

    @property
    def default_asr_model_path(self) -> Path:
        return self.asr_dir / "models" / "nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf"

    def resolved_neo4j_password(self, *, create: bool = False) -> str:
        if self.neo4j_password:
            return self.neo4j_password
        if self.neo4j_password_path.is_file():
            return self.neo4j_password_path.read_text(encoding="utf-8").strip()
        if not create:
            return ""
        self.data_dir.mkdir(parents=True, exist_ok=True)
        password = secrets.token_urlsafe(32)
        self.neo4j_password_path.write_text(password, encoding="utf-8")
        with suppress(OSError):
            self.neo4j_password_path.chmod(0o600)
        return password

    @property
    def platform_name(self) -> str:
        return platform.system().lower()


@lru_cache
def get_settings() -> Settings:
    return Settings()
