"""RAG layer errors. Every message here reaches the UI or the model's context,
so each one must say what the reader should do next, not merely that something failed.
"""

from __future__ import annotations

__all__ = [
    "ConfigError",
    "EmbedError",
    "ExtractError",
    "GraphError",
    "RagError",
    "RerankError",
    "UnsupportedFile",
    "VectorStoreError",
]


class RagError(Exception):
    """Root of every error in this layer; catching this catches all of them."""


class ConfigError(RagError):
    """Configuration missing or contradictory. The user can fix it in Settings."""


class ExtractError(RagError):
    """A file yielded no text; always carries the path, since the user just dropped twenty files."""

    def __init__(self, path: str, reason: str) -> None:
        super().__init__(f"{path}: {reason}")
        self.path = path
        self.reason = reason


class UnsupportedFile(ExtractError):
    """Format outside the readable set. Distinct from `ExtractError` because retrying will not help."""


class EmbedError(RagError):
    """The embedding server did not answer, or returned something unusable."""


class RerankError(RagError):
    """Reranker failure; always caught upstream, since reranking improves results rather than producing them."""


class VectorStoreError(RagError):
    """Qdrant is unreachable, or the collection is in an unusable state."""


class GraphError(RagError):
    """The graph store is unreachable or refused a query; caught upstream, so `auto` falls back to `hybrid`."""
