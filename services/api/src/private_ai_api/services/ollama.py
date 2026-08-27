from __future__ import annotations

import json
import math
from collections.abc import AsyncIterator
from datetime import datetime
from typing import Any

import httpx

from private_ai_api.schemas import ChatRequest, ModelInfo, ModelState
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.provider import (
    GRAPH_SCHEMA,
    ProviderUnavailable,
    decode_json_object,
    graph_messages,
    infer_capabilities,
    normalize_graph_result,
)


class OllamaUnavailable(ProviderUnavailable):
    pass


class OllamaClient:
    def __init__(
        self,
        base_url: str,
        timeout: float = 60.0,
        *,
        gpu_leases: GpuLeaseManager | None = None,
        model_overhead_ratio: float = 1.1,
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.gpu_leases = gpu_leases
        self.model_overhead_ratio = max(1.0, model_overhead_ratio)
        self.transport = transport
        self._prepared_models: set[str] = set()

    def _client(self, timeout: float | None) -> httpx.AsyncClient:
        return httpx.AsyncClient(timeout=timeout, transport=self.transport)

    async def health(self) -> bool:
        try:
            async with self._client(2.0) as client:
                response = await client.get(f"{self.base_url}/api/ps")
                response.raise_for_status()
                if self.gpu_leases:
                    reservations = {
                        self._owner(str(model["name"])): int(model.get("size_vram", 0))
                        for model in response.json().get("models", [])
                    }
                    await self.gpu_leases.synchronize("ollama:", reservations)
                    self._prepared_models = self.gpu_leases.owners("ollama:")
                return True
        except httpx.HTTPError:
            return False

    async def list_models(self) -> list[ModelInfo]:
        try:
            async with self._client(self.timeout) as client:
                installed, running = await self._model_responses(client)
        except httpx.HTTPError as exc:
            raise OllamaUnavailable(str(exc)) from exc

        running_by_name = {item["name"]: item for item in running.get("models", [])}
        result: list[ModelInfo] = []
        for item in installed.get("models", []):
            name = item["name"]
            active = running_by_name.get(name)
            details = item.get("details", {})
            capabilities = self._capabilities(f"{name} {details.get('family', '')}")
            result.append(
                ModelInfo(
                    name=name,
                    model_type="embedding" if capabilities == ["embedding"] else "language",
                    state=ModelState.LOADED if active else ModelState.UNLOADED,
                    size_bytes=item.get("size", 0),
                    vram_bytes=(active or {}).get("size_vram", 0),
                    quantization=details.get("quantization_level"),
                    modified_at=self._parse_datetime(item.get("modified_at")),
                    capabilities=capabilities,
                )
            )
        await self._synchronize_running_models(result)
        return result

    @staticmethod
    def _owner(name: str) -> str:
        return f"ollama:{name}"

    async def _synchronize_running_models(self, models: list[ModelInfo]) -> None:
        if not self.gpu_leases:
            return
        reservations = {
            self._owner(model.name): model.vram_bytes
            or math.ceil(model.size_bytes * self.model_overhead_ratio)
            for model in models
            if model.state == ModelState.LOADED
        }
        await self.gpu_leases.synchronize("ollama:", reservations)
        self._prepared_models = self.gpu_leases.owners("ollama:")

    async def _reserve_model(self, requested_name: str) -> str | None:
        if not self.gpu_leases:
            return None
        requested_owner = self._owner(requested_name)
        if requested_owner in self._prepared_models:
            return requested_owner
        models = await self.list_models()
        model = next(
            (
                candidate
                for candidate in models
                if candidate.name == requested_name
                or candidate.name.removesuffix(":latest") == requested_name
            ),
            None,
        )
        if model is None:
            return None
        owner = self._owner(model.name)
        bytes_required = model.vram_bytes or math.ceil(
            model.size_bytes * self.model_overhead_ratio
        )
        await self.gpu_leases.reserve(owner, bytes_required)
        self._prepared_models.add(owner)
        return owner

    async def _mark_model_loaded(self, owner: str | None) -> None:
        if self.gpu_leases and owner:
            await self.gpu_leases.mark_observed(owner)

    async def _reconcile_after_failure(self, owner: str | None) -> None:
        if not self.gpu_leases or not owner:
            return
        try:
            await self.list_models()
        except OllamaUnavailable:
            await self.gpu_leases.release(owner)
            self._prepared_models.discard(owner)

    async def _refresh_inventory_best_effort(self) -> None:
        try:
            await self.list_models()
        except OllamaUnavailable:
            return

    async def _model_responses(
        self, client: httpx.AsyncClient
    ) -> tuple[dict[str, Any], dict[str, Any]]:
        tags_response = await client.get(f"{self.base_url}/api/tags")
        tags_response.raise_for_status()
        running_response = await client.get(f"{self.base_url}/api/ps")
        running_response.raise_for_status()
        return tags_response.json(), running_response.json()

    async def pull(self, name: str) -> AsyncIterator[dict[str, Any]]:
        payload = {"name": name, "stream": True}
        try:
            async with (
                self._client(None) as client,
                client.stream(
                    "POST",
                    f"{self.base_url}/api/pull",
                    json=payload,
                ) as response,
            ):
                response.raise_for_status()
                async for line in response.aiter_lines():
                    if line:
                        yield json.loads(line)
        except httpx.HTTPError as exc:
            raise OllamaUnavailable(str(exc)) from exc

    async def unload(self, name: str) -> None:
        await self._post("/api/generate", {"model": name, "keep_alive": 0})
        if self.gpu_leases:
            await self.gpu_leases.release(self._owner(name))
            self._prepared_models.discard(self._owner(name))
            await self._refresh_inventory_best_effort()

    async def delete(self, name: str) -> None:
        try:
            async with self._client(self.timeout) as client:
                response = await client.request(
                    "DELETE",
                    f"{self.base_url}/api/delete",
                    json={"name": name},
                )
                response.raise_for_status()
        except httpx.HTTPError as exc:
            raise OllamaUnavailable(str(exc)) from exc
        if self.gpu_leases:
            await self.gpu_leases.release(self._owner(name))
            self._prepared_models.discard(self._owner(name))
            await self._refresh_inventory_best_effort()

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        owner = await self._reserve_model(request.model)
        try:
            result = await self._post("/api/chat", request.model_dump())
        except OllamaUnavailable:
            await self._reconcile_after_failure(owner)
            raise
        await self._mark_model_loaded(owner)
        return result

    async def chat_stream(self, request: ChatRequest) -> AsyncIterator[dict[str, Any]]:
        owner = await self._reserve_model(request.model)
        payload = request.model_dump()
        payload["stream"] = True
        connected = False
        try:
            async with (
                self._client(None) as client,
                client.stream(
                    "POST",
                    f"{self.base_url}/api/chat",
                    json=payload,
                ) as response,
            ):
                response.raise_for_status()
                connected = True
                async for line in response.aiter_lines():
                    if line:
                        yield json.loads(line)
        except httpx.HTTPError as exc:
            await self._reconcile_after_failure(owner)
            raise OllamaUnavailable(str(exc)) from exc
        finally:
            if connected:
                await self._mark_model_loaded(owner)

    async def embed(self, model: str, inputs: list[str]) -> list[list[float]]:
        owner = await self._reserve_model(model)
        try:
            result = await self._post(
                "/api/embed",
                {"model": model, "input": inputs, "keep_alive": "5m"},
            )
        except OllamaUnavailable:
            await self._reconcile_after_failure(owner)
            raise
        await self._mark_model_loaded(owner)
        embeddings = result.get("embeddings")
        if not isinstance(embeddings, list) or len(embeddings) != len(inputs):
            raise OllamaUnavailable("Ollama returned an invalid embedding response")
        return embeddings

    async def extract_graph(
        self,
        model: str,
        content: str,
    ) -> dict[str, list[dict[str, str]]]:
        messages = graph_messages(content)
        owner = await self._reserve_model(model)
        try:
            response = await self._post(
                "/api/chat",
                {
                    "model": model,
                    "stream": False,
                    "format": GRAPH_SCHEMA,
                    "options": {"temperature": 0},
                    "messages": messages,
                },
            )
        except OllamaUnavailable:
            await self._reconcile_after_failure(owner)
            raise
        await self._mark_model_loaded(owner)
        try:
            parsed = self._decode_json_object(response)
        except ProviderUnavailable:
            fallback = await self._post(
                "/api/chat",
                {
                    "model": model,
                    "stream": False,
                    "format": "json",
                    "options": {"temperature": 0},
                    "messages": messages,
                },
            )
            parsed = self._decode_json_object(fallback)
        return normalize_graph_result(parsed)

    @staticmethod
    def _decode_json_object(response: dict[str, Any]) -> dict[str, Any]:
        return decode_json_object(response.get("message", {}).get("content", ""))

    async def _post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        try:
            async with self._client(self.timeout) as client:
                response = await client.post(f"{self.base_url}{path}", json=payload)
                response.raise_for_status()
                return response.json()
        except httpx.HTTPError as exc:
            raise OllamaUnavailable(str(exc)) from exc

    @staticmethod
    def _parse_datetime(value: str | None) -> datetime | None:
        if not value:
            return None
        return datetime.fromisoformat(value.replace("Z", "+00:00"))

    @staticmethod
    def _capabilities(family: str) -> list[str]:
        return infer_capabilities(family)
