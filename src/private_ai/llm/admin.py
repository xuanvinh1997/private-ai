"""Ollama model lifecycle — the half of a provider LangChain has no concept of.

LangChain models an inference call and nothing else, so pulling a model, listing what
is installed, seeing what is resident in VRAM, evicting it and deleting it all stay on
raw httpx here. An OpenAI-compatible provider hosts its models elsewhere and answers
every one of these with ``ProviderReadOnly``.
"""

from __future__ import annotations

import asyncio
import json
import math
from collections.abc import AsyncIterator
from datetime import datetime
from typing import TYPE_CHECKING, Any

import httpx

from private_ai.core.schemas import ModelInfo, ModelState
from private_ai.llm import NoProviderConfigured, ProviderReadOnly, ProviderUnavailable
from private_ai.llm.capabilities import infer_capabilities, normalize_ollama_capabilities

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.gpu_lease import GpuLeaseManager
    from private_ai.llm.registry import ProviderConfig, ProviderRegistry

__all__ = ["ModelAdmin", "pull_fraction"]


def pull_fraction(event: dict[str, Any]) -> float:
    """0.0–1.0 for one ``/api/pull`` line, for a ``ProgressSink``."""
    total = event.get("total")
    completed = event.get("completed")
    if not isinstance(total, int | float) or total <= 0:
        return 0.0
    if not isinstance(completed, int | float):
        return 0.0
    return max(0.0, min(1.0, float(completed) / float(total)))


class ModelAdmin:
    """Ollama's admin endpoints, aimed at whichever provider is currently active."""

    def __init__(
        self,
        registry: ProviderRegistry,
        *,
        timeout: float = 60.0,
        gpu_leases: GpuLeaseManager | None = None,
        model_overhead_ratio: float = 1.1,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.registry = registry
        self.timeout = timeout
        self.gpu_leases = gpu_leases
        self.model_overhead_ratio = max(1.0, model_overhead_ratio)
        self.transport = transport

    # --- provider resolution ---------------------------------------------

    def provider(self, action: str) -> ProviderConfig:
        config = self.registry.active_config()
        if config is None:
            raise NoProviderConfigured("Chưa cấu hình nhà cung cấp AI nào")
        if config.kind != "ollama":
            raise ProviderReadOnly(f"{config.name} lưu mô hình từ xa; {action} không áp dụng")
        return config

    def _base_url(self, action: str) -> str:
        return self.provider(action).base_url.rstrip("/")

    def supports_lifecycle(self) -> bool:
        config = self.registry.active_config()
        return config is not None and config.kind == "ollama"

    def _client(self, timeout: float | None) -> httpx.AsyncClient:
        return httpx.AsyncClient(timeout=timeout, transport=self.transport)

    # --- reads ------------------------------------------------------------

    async def health(self) -> bool:
        try:
            base_url = self._base_url("kiểm tra tình trạng")
        except (NoProviderConfigured, ProviderReadOnly):
            return False
        try:
            async with self._client(2.0) as client:
                response = await client.get(f"{base_url}/api/ps")
                response.raise_for_status()
        except httpx.HTTPError:
            return False
        return True

    async def tags(self) -> list[dict[str, Any]]:
        payload = await self._get("/api/tags", "liệt kê mô hình")
        models = payload.get("models")
        return models if isinstance(models, list) else []

    async def ps(self) -> list[dict[str, Any]]:
        payload = await self._get("/api/ps", "xem mô hình đang chạy")
        models = payload.get("models")
        return models if isinstance(models, list) else []

    async def show(self, name: str) -> dict[str, Any]:
        return await self._post("/api/show", {"model": name, "verbose": False}, "xem chi tiết")

    async def list_models(self) -> list[ModelInfo]:
        base_url = self._base_url("liệt kê mô hình")
        try:
            async with self._client(self.timeout) as client:
                tags_response = await client.get(f"{base_url}/api/tags")
                tags_response.raise_for_status()
                running_response = await client.get(f"{base_url}/api/ps")
                running_response.raise_for_status()
                installed = tags_response.json().get("models") or []
                running = running_response.json().get("models") or []
                reported = await self._reported_capabilities(client, base_url, installed)
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc

        running_by_name = {str(item.get("name", "")): item for item in running}
        result: list[ModelInfo] = []
        for item in installed:
            name = str(item.get("name", ""))
            if not name:
                continue
            active = running_by_name.get(name)
            details = item.get("details") or {}
            capabilities = reported.get(name) or infer_capabilities(
                f"{name} {details.get('family', '')}"
            )
            result.append(
                ModelInfo(
                    name=name,
                    model_type="embedding" if capabilities == ["embedding"] else "language",
                    state=ModelState.LOADED if active else ModelState.UNLOADED,
                    size_bytes=int(item.get("size") or 0),
                    vram_bytes=int((active or {}).get("size_vram") or 0),
                    quantization=details.get("quantization_level"),
                    modified_at=self._parse_datetime(item.get("modified_at")),
                    capabilities=capabilities,
                )
            )
        return result

    async def _reported_capabilities(
        self,
        client: httpx.AsyncClient,
        base_url: str,
        models: list[dict[str, Any]],
    ) -> dict[str, list[str]]:
        """Read Ollama's authoritative metadata; old servers fall back to name inference."""
        result: dict[str, list[str]] = {}
        for item in models:
            name = str(item.get("name", "")).strip()
            if not name:
                continue
            try:
                response = await client.post(
                    f"{base_url}/api/show",
                    json={"model": name, "verbose": False},
                )
                response.raise_for_status()
            except httpx.HTTPError:
                continue
            capabilities = normalize_ollama_capabilities(response.json().get("capabilities"))
            if capabilities:
                result[name] = capabilities
        return result

    def required_bytes(self, model: ModelInfo) -> int:
        """What a lease for this model should reserve: measured VRAM, else file size + headroom."""
        return model.vram_bytes or math.ceil(model.size_bytes * self.model_overhead_ratio)

    # --- writes -----------------------------------------------------------

    async def pull(
        self,
        name: str,
        *,
        cancel: asyncio.Event | None = None,
    ) -> AsyncIterator[dict[str, Any]]:
        """Stream Ollama's NDJSON download progress, one decoded line at a time.

        No timeout: a multi-gigabyte pull legitimately outlives any request budget. Closing
        the generator — or setting ``cancel`` — tears the connection down, which is what
        aborts the download on the server side.
        """
        base_url = self._base_url("tải mô hình")
        try:
            async with (
                self._client(None) as client,
                client.stream(
                    "POST",
                    f"{base_url}/api/pull",
                    json={"name": name, "stream": True},
                ) as response,
            ):
                response.raise_for_status()
                async for line in response.aiter_lines():
                    if cancel is not None and cancel.is_set():
                        return
                    if not line:
                        continue
                    try:
                        yield json.loads(line)
                    except json.JSONDecodeError:
                        continue
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc

    async def unload(self, name: str) -> None:
        """Evict a resident model. Ollama has no verb for this beyond a zero keep-alive."""
        await self._post("/api/generate", {"model": name, "keep_alive": 0}, "gỡ mô hình")
        await self._release(name)

    async def delete(self, name: str) -> None:
        base_url = self._base_url("xoá mô hình")
        try:
            async with self._client(self.timeout) as client:
                response = await client.request(
                    "DELETE",
                    f"{base_url}/api/delete",
                    json={"name": name},
                )
                response.raise_for_status()
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc
        await self._release(name)

    async def _release(self, name: str) -> None:
        if not self.gpu_leases:
            return
        from private_ai.llm.leases import owner_for, synchronize_running_models

        await self.gpu_leases.release(owner_for(name))
        # The server may have evicted more than we asked for, so re-read the truth.
        try:
            await synchronize_running_models(self, self.gpu_leases)
        except ProviderUnavailable:
            return

    # --- transport --------------------------------------------------------

    async def _get(self, path: str, action: str) -> dict[str, Any]:
        base_url = self._base_url(action)
        try:
            async with self._client(self.timeout) as client:
                response = await client.get(f"{base_url}{path}")
                response.raise_for_status()
                return response.json()
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc

    async def _post(self, path: str, payload: dict[str, Any], action: str) -> dict[str, Any]:
        base_url = self._base_url(action)
        try:
            async with self._client(self.timeout) as client:
                response = await client.post(f"{base_url}{path}", json=payload)
                response.raise_for_status()
                return response.json()
        except httpx.HTTPError as exc:
            raise ProviderUnavailable(str(exc)) from exc

    @staticmethod
    def _parse_datetime(value: Any) -> datetime | None:
        if not value:
            return None
        try:
            return datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        except ValueError:
            return None
