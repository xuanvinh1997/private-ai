"""Shared fixtures.

Everything here builds the *real* services against a temporary data directory. The only
things faked are the two that would otherwise reach the network: the chat model and the
embeddings client. Both are deterministic, so a similarity ordering or a token sequence
asserted here means the same thing on every machine.
"""

from __future__ import annotations

import json
import re
from collections.abc import Iterator, Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from uuid import uuid4

import pytest
from langchain_core.embeddings import Embeddings
from langchain_core.language_models.fake_chat_models import GenericFakeChatModel
from langchain_core.messages import AIMessage, AIMessageChunk
from langchain_core.messages.tool import tool_call_chunk
from langchain_core.outputs import ChatGenerationChunk
from pydantic import Field

from private_ai.config import Settings
from private_ai.core.bootstrap import build_services
from private_ai.core.database import Database
from private_ai.core.services import AppServices

EMBEDDING_MODEL = "test-embedding"
EMBEDDING_DIMENSIONS = 24
_TOKEN = re.compile(r"[^\W_]+", re.UNICODE)


# --- fakes ----------------------------------------------------------------


class HashingEmbeddings(Embeddings):
    """A bag-of-words embedding: deterministic, and its cosine ordering is explainable.

    Two texts that share words point in similar directions; two that share none are
    orthogonal. That is enough to assert a ranking without pinning magic numbers.
    """

    dimensions = EMBEDDING_DIMENSIONS

    def __init__(self) -> None:
        self.calls: list[list[str]] = []

    def _vector(self, text: str) -> list[float]:
        vector = [0.0] * self.dimensions
        for token in _TOKEN.findall(text.casefold()):
            vector[sum(ord(char) for char in token) % self.dimensions] += 1.0
        if not any(vector):
            vector[0] = 1.0
        return vector

    def embed_documents(self, texts: list[str]) -> list[list[float]]:
        self.calls.append(list(texts))
        return [self._vector(text) for text in texts]

    def embed_query(self, text: str) -> list[float]:
        return self._vector(text)

    async def aembed_documents(self, texts: list[str]) -> list[list[float]]:
        return self.embed_documents(texts)

    async def aembed_query(self, text: str) -> list[float]:
        return self.embed_query(text)


class ScriptedChatModel(GenericFakeChatModel):
    """``GenericFakeChatModel`` that can also stream tool calls and accept ``bind_tools``.

    The stock fake drops ``tool_calls`` on the streaming path and refuses ``bind_tools``
    outright, which is precisely the half of the agent loop worth testing. Every round
    appends to ``bound_tools`` — ``None`` when the round was offered no tools at all —
    so a test can assert that the last round was made to answer.
    """

    bound_tools: list[list[str] | None] = Field(default_factory=list)

    def bind_tools(self, tools: Sequence[Any], **kwargs: Any) -> ScriptedChatModel:
        self.bound_tools.append([getattr(tool, "name", str(tool)) for tool in tools])
        return self

    def _generate(self, messages, stop=None, run_manager=None, **kwargs):  # type: ignore[no-untyped-def]
        return super()._generate(messages, stop=stop, run_manager=run_manager, **kwargs)

    def _stream(self, messages, stop=None, run_manager=None, **kwargs):  # type: ignore[no-untyped-def]
        result = self._generate(messages, stop=stop, run_manager=run_manager)
        message = result.generations[0].message
        if not isinstance(message, AIMessage) or not message.tool_calls:
            # Split on whitespace, keeping it, so a cancellation lands mid-answer the way
            # it does against a real provider.
            for piece in re.split(r"(\s)", str(message.content)):
                if piece:
                    yield ChatGenerationChunk(message=AIMessageChunk(id=message.id, content=piece))
            return
        yield ChatGenerationChunk(
            message=AIMessageChunk(
                id=message.id,
                content=message.content,
                tool_call_chunks=[
                    tool_call_chunk(
                        name=str(call["name"]),
                        args=json.dumps(call.get("args") or {}, ensure_ascii=False),
                        id=str(call.get("id") or uuid4()),
                        index=index,
                    )
                    for index, call in enumerate(message.tool_calls)
                ],
            )
        )


def scripted_model(messages: Sequence[AIMessage | str]) -> ScriptedChatModel:
    return ScriptedChatModel(messages=iter(list(messages)))


# --- fixtures -------------------------------------------------------------


@pytest.fixture
def settings(tmp_path: Path) -> Settings:
    """A Settings that touches nothing outside ``tmp_path``."""
    return Settings(
        data_dir=tmp_path / "data",
        embedding_model=EMBEDDING_MODEL,
        embedding_enabled=True,
        # No token file, no auth handshake: the tests mount servers in process.
        mcp_require_auth=False,
        mcp_external_servers="",
        inline_ingestion=True,
        skill_paths="",
        file_roots="",
        agent_max_iterations=4,
        asr_enabled=False,
        gpu_capacity_bytes=8 * 1024**3,
    )


@pytest.fixture
def database(settings: Settings) -> Iterator[Database]:
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    store = Database(settings.database_path)
    store.initialize()
    try:
        yield store
    finally:
        store.close()


@pytest.fixture
def fake_embeddings() -> HashingEmbeddings:
    return HashingEmbeddings()


@pytest.fixture
def services(settings: Settings, fake_embeddings: HashingEmbeddings) -> Iterator[AppServices]:
    """The real ``AppServices``, with only the two network-bound clients replaced."""
    built = build_services(settings)
    built.models.embeddings = lambda model="": fake_embeddings  # type: ignore[method-assign]
    built.models.default_model = lambda task: (  # type: ignore[method-assign]
        EMBEDDING_MODEL if task == "embedding" else "test-chat"
    )
    try:
        yield built
    finally:
        built.database.close()


def _make_workspace(services: AppServices, name: str) -> str:
    now = datetime.now(UTC).isoformat()
    identifier = str(uuid4())
    services.database.execute(
        "INSERT INTO workspaces(id, name, description, created_at, updated_at) "
        "VALUES (?, ?, '', ?, ?)",
        (identifier, name, now, now),
    )
    return identifier


@pytest.fixture
def workspace_id(services: AppServices) -> str:
    """A workspace of our own, so the seeded starter rows are never load-bearing."""
    return _make_workspace(services, "Kiểm thử")


@pytest.fixture
def other_workspace_id(services: AppServices) -> str:
    """The neighbour a workspace boundary is asserted against."""
    return _make_workspace(services, "Kiểm thử khác")


@pytest.fixture
def qapp() -> Iterator[Any]:
    """A ``QApplication`` for widget tests; skips cleanly when Qt is unavailable."""
    import os

    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    widgets = pytest.importorskip(
        "PySide6.QtWidgets",
        reason="PySide6 is not installed; Qt tests are skipped",
    )
    existing = widgets.QApplication.instance()
    application = existing or widgets.QApplication([])
    try:
        yield application
    finally:
        if existing is None:
            application.quit()


# --- helpers usable from any test ----------------------------------------


def insert_document(
    database: Database,
    workspace_id: str,
    filename: str,
    text: str = "",
    *,
    status: str = "ready",
    index_mode: str = "simple",
) -> str:
    """A ``documents`` row without going through ingestion."""
    document_id = str(uuid4())
    now = datetime.now(UTC).isoformat()
    database.execute(
        """
        INSERT INTO documents(
            id, workspace_id, filename, media_type, sha256, byte_size, status,
            source_path, extracted_text, index_mode, created_at, updated_at
        ) VALUES (?, ?, ?, 'text/plain', ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            document_id,
            workspace_id,
            filename,
            f"sha-{document_id}",
            len(text.encode()),
            status,
            f"/tmp/{document_id}/{filename}",  # noqa: S108 - never opened
            text or None,
            index_mode,
            now,
            now,
        ),
    )
    return document_id
