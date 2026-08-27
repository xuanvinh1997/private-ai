from __future__ import annotations

from datetime import UTC, datetime
from enum import StrEnum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


def utc_now() -> datetime:
    return datetime.now(UTC)


class MemoryType(StrEnum):
    PREFERENCE = "preference"
    FACT = "fact"
    EPISODIC = "episodic"


class MemoryCreate(BaseModel):
    user_id: str = "local-user"
    type: MemoryType
    content: str = Field(min_length=1, max_length=10_000)
    source: str = "user"
    confidence: float = Field(default=1.0, ge=0, le=1)
    expires_at: datetime | None = None


class MemoryRecord(MemoryCreate):
    id: str = Field(default_factory=lambda: str(uuid4()))
    enabled: bool = True
    created_at: datetime = Field(default_factory=utc_now)
    updated_at: datetime = Field(default_factory=utc_now)


class ModelState(StrEnum):
    INSTALLED = "installed"
    LOADED = "loaded"
    UNLOADED = "unloaded"
    DOWNLOADING = "downloading"
    FAILED = "failed"


class ModelInfo(BaseModel):
    name: str
    model_type: str = "language"
    state: ModelState = ModelState.INSTALLED
    size_bytes: int = 0
    vram_bytes: int = 0
    quantization: str | None = None
    modified_at: datetime | None = None
    capabilities: list[str] = Field(default_factory=list)
    runtime: str = "ollama"
    sha256: str | None = None
    default_for: list[str] = Field(default_factory=list)
    error: str | None = None


class PullRequest(BaseModel):
    name: str = Field(min_length=1)


class DefaultModelRequest(BaseModel):
    model: str = Field(min_length=1, max_length=240)


class PreferencesRecord(BaseModel):
    ocr_enabled: bool = True


class PreferencesUpdate(BaseModel):
    ocr_enabled: bool | None = None


class ProviderKind(StrEnum):
    OLLAMA = "ollama"
    OPENAI = "openai"


class ProviderCreate(BaseModel):
    name: str = Field(min_length=1, max_length=120)
    kind: ProviderKind = ProviderKind.OPENAI
    base_url: str = Field(min_length=1, max_length=500)
    api_key: str = Field(default="", max_length=500)
    enabled: bool = True


class ProviderUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=120)
    base_url: str | None = Field(default=None, min_length=1, max_length=500)
    api_key: str | None = Field(default=None, max_length=500)
    enabled: bool | None = None


class ProviderRecord(BaseModel):
    id: str
    name: str
    kind: ProviderKind
    base_url: str
    has_api_key: bool = False
    enabled: bool = True
    builtin: bool = False
    active: bool = False
    created_at: str | None = None
    updated_at: str | None = None


class ProviderProbe(BaseModel):
    """A connection check that can run against a saved provider or an unsaved draft."""

    kind: ProviderKind = ProviderKind.OPENAI
    base_url: str = Field(min_length=1, max_length=500)
    api_key: str = Field(default="", max_length=500)


class ProviderProbeResult(BaseModel):
    reachable: bool
    model_count: int = 0
    models: list[str] = Field(default_factory=list)
    detail: str | None = None


class ChatMessage(BaseModel):
    role: str
    content: str


class ChatRequest(BaseModel):
    model: str
    messages: list[ChatMessage]
    stream: bool = False
    options: dict[str, Any] = Field(default_factory=dict)


class WorkspaceCreate(BaseModel):
    name: str = Field(min_length=1, max_length=120)
    description: str = Field(default="", max_length=500)


class WorkspaceUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=120)
    description: str | None = Field(default=None, max_length=500)


class WorkspaceRecord(WorkspaceCreate):
    id: str
    created_at: datetime
    updated_at: datetime
    conversation_count: int = 0


class ConversationCreate(BaseModel):
    title: str = Field(default="Cuộc trò chuyện mới", min_length=1, max_length=160)
    model: str | None = None


class ConversationRecord(ConversationCreate):
    id: str
    workspace_id: str
    created_at: datetime
    updated_at: datetime
    message_count: int = 0


class MessageRecord(BaseModel):
    id: str
    conversation_id: str
    role: str
    content: str
    created_at: datetime


class ConversationDetail(ConversationRecord):
    messages: list[MessageRecord] = Field(default_factory=list)


class ConversationChatRequest(BaseModel):
    content: str = Field(min_length=1, max_length=100_000)
    model: str = Field(min_length=1)
