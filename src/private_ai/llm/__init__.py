"""Model providers and the LangChain layer built on top of them.

The failure modes here are shared by the registry, the router and the Ollama admin
client, so they live on the package rather than in any one module — importing them
from a submodule would make every caller depend on which module happens to raise.
"""

from __future__ import annotations

from private_ai.core.gpu_lease import InsufficientVram

__all__ = [
    "InsufficientVram",
    "NoProviderConfigured",
    "ProviderReadOnly",
    "ProviderUnavailable",
    "UnknownProvider",
]


class ProviderUnavailable(RuntimeError):
    """The selected AI provider could not serve the request."""


class NoProviderConfigured(ProviderUnavailable):
    """Every provider has been removed, so there is nowhere to send the request."""


class ProviderReadOnly(RuntimeError):
    """The provider hosts its models remotely, so local lifecycle actions do not apply."""


class UnknownProvider(LookupError):
    """No provider row carries the requested id."""
