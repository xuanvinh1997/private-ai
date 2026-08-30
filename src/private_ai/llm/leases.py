"""GPU accounting for LangChain model calls.

The old httpx client reserved VRAM inline, around every request it made. LangChain owns
the request now, so the same three steps — reserve before the model runs, confirm the
model really loaded, put the books back after a failure — hang off a callback handler
instead. Owner strings stay ``ollama:<model>`` so a ``synchronize`` from ``/api/ps``
still lines up with what we reserved.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from langchain_core.callbacks import AsyncCallbackHandler

from private_ai.llm import ProviderUnavailable

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.gpu_lease import GpuLeaseManager
    from private_ai.llm.admin import ModelAdmin

__all__ = ["OWNER_PREFIX", "GpuLeaseCallback", "owner_for", "synchronize_running_models"]

OWNER_PREFIX = "ollama:"


def owner_for(model: str) -> str:
    return f"{OWNER_PREFIX}{model}"


async def synchronize_running_models(
    admin: ModelAdmin,
    leases: GpuLeaseManager,
) -> None:
    """Replace the observed Ollama leases with what the server says is actually resident."""
    reservations: dict[str, int] = {}
    for item in await admin.ps():
        name = str(item.get("name", "")).strip()
        if not name:
            continue
        reservations[owner_for(name)] = int(item.get("size_vram") or 0)
    await leases.synchronize(OWNER_PREFIX, reservations)


class GpuLeaseCallback(AsyncCallbackHandler):
    """Reserves capacity for one model before it runs, and releases it if the run fails."""

    # Without this LangChain logs a callback failure and carries on, which would let a call
    # proceed after ``reserve`` refused it — exactly the over-commit the leases prevent.
    raise_error = True

    def __init__(
        self,
        *,
        leases: GpuLeaseManager,
        admin: ModelAdmin,
        model: str,
    ) -> None:
        self.leases = leases
        self.admin = admin
        self.model = model
        self._owner: str | None = None

    async def on_chat_model_start(self, serialized: dict, messages: Any, **kwargs: Any) -> None:
        await self._reserve()

    async def on_llm_start(self, serialized: dict, prompts: list[str], **kwargs: Any) -> None:
        await self._reserve()

    async def on_llm_end(self, response: Any, **kwargs: Any) -> None:
        if self._owner:
            await self.leases.mark_observed(self._owner)

    async def on_llm_error(self, error: BaseException, **kwargs: Any) -> None:
        await self._reconcile_after_failure()

    async def _reserve(self) -> None:
        owner = owner_for(self.model)
        # An already-leased model is resident; re-reserving would only re-measure it.
        if owner in self.leases.owners(OWNER_PREFIX):
            self._owner = owner
            return
        try:
            installed = await self.admin.list_models()
        except ProviderUnavailable:
            # Nothing to account for if we cannot even read the inventory.
            self._owner = None
            return
        match = next(
            (
                candidate
                for candidate in installed
                if candidate.name == self.model
                or candidate.name.removesuffix(":latest") == self.model
            ),
            None,
        )
        if match is None:
            self._owner = None
            return
        owner = owner_for(match.name)
        await self.leases.reserve(owner, self.admin.required_bytes(match))
        self._owner = owner

    async def _reconcile_after_failure(self) -> None:
        """A failed call may or may not have loaded the model; ask the server which."""
        owner = self._owner
        if not owner:
            return
        try:
            await self.admin.list_models()
        except ProviderUnavailable:
            await self.leases.release(owner)
            self._owner = None
