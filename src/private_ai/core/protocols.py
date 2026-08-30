"""Structural contracts shared across subsystems.

These exist so the retrieval strategies, MCP servers and the UI can be written and
tested against a shape rather than against each other's imports. Nothing inherits
from them at runtime; they are ``Protocol`` types checked statically.
"""

from __future__ import annotations

from collections.abc import AsyncIterator, Sequence
from typing import Any, Protocol, runtime_checkable

from langchain_core.documents import Document


@runtime_checkable
class Retrieval(Protocol):
    """One retrieval strategy.

    Every strategy is also exposed as its own MCP server, so ``name`` doubles as the
    server identity and ``description`` as the tool description the model reads when
    deciding whether this strategy fits the question.
    """

    name: str
    description: str

    async def retrieve(
        self,
        query: str,
        *,
        workspace_id: str,
        limit: int = 5,
        **options: Any,
    ) -> list[Document]:
        """Return scored, deduplicated documents. Never raises for an empty index."""
        ...


@runtime_checkable
class DocumentSource(Protocol):
    """A loader that turns one file on disk into LangChain documents."""

    def handles(self, path: str, media_type: str) -> bool: ...

    async def load(self, path: str, **options: Any) -> list[Document]: ...


@runtime_checkable
class ProgressSink(Protocol):
    """Where long-running work reports itself.

    Ingestion, model pulls and agent runs all report through this, which is what lets
    the same pipeline drive a Qt progress bar, a worker log line and an MCP
    notification without knowing which it is talking to.
    """

    def __call__(self, stage: str, progress: float, detail: str = "") -> None: ...


@runtime_checkable
class ChatModelFactory(Protocol):
    """Builds LangChain chat models for a task, honouring the active provider."""

    def chat_model(self, model: str = "", **kwargs: Any) -> Any: ...

    def embeddings(self, model: str = "") -> Any: ...


class AgentEvent(Protocol):
    """One item on the agent's output stream."""

    type: str  # "token" | "tool_start" | "tool_end" | "notice" | "error" | "final"
    data: dict[str, Any]


@runtime_checkable
class AgentStream(Protocol):
    def __aiter__(self) -> AsyncIterator[AgentEvent]: ...


@runtime_checkable
class ToolProvider(Protocol):
    """Anything that can hand LangChain tools to the agent."""

    async def tools(self, *, allow: Sequence[str] | None = None) -> list[Any]: ...
