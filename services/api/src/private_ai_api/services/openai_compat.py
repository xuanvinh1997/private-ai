from __future__ import annotations

import json
from collections.abc import AsyncIterator
from datetime import UTC, datetime
from typing import Any

import httpx

from private_ai_api.schemas import ChatRequest, ModelInfo, ModelState
from private_ai_api.services.provider import (
    ProviderReadOnly,
    ProviderUnavailable,
    decode_json_object,
    graph_messages,
    infer_capabilities,
    normalize_graph_result,
)

# Ollama names its sampling knobs differently from the OpenAI wire format, and the app
# speaks Ollama everywhere else. Only the options with an exact counterpart are forwarded.
OPTION_ALIASES = {
    "temperature": "temperature",
    "top_p": "top_p",
    "seed": "seed",
    "stop": "stop",
    "num_predict": "max_tokens",
    "max_tokens": "max_tokens",
    "frequency_penalty": "frequency_penalty",
    "presence_penalty": "presence_penalty",
}

_DONE = object()


class OpenAICompatUnavailable(ProviderUnavailable):
    pass


class OpenAICompatClient:
    """Talks to any host that speaks the OpenAI REST dialect, in Ollama's response shape.

    Routers, the frontend and the ingestion pipeline all consume Ollama-shaped payloads, so
    translation happens here rather than spreading provider branches through the callers.
    """

    def __init__(
        self,
        base_url: str,
        api_key: str = "",
        timeout: float = 60.0,
        *,
        label: str = "openai",
        transport: httpx.AsyncBaseTransport | None = None,
    ) -> None:
        self.base_url = self._normalize_base_url(base_url)
        self.api_key = api_key.strip()
        self.timeout = timeout
        self.label = label
        self.transport = transport

    @staticmethod
    def _normalize_base_url(base_url: str) -> str:
        """Accept either the API root or the bare host, the way the OpenAI SDKs do."""
        value = base_url.strip().rstrip("/")
        if not value:
            raise ValueError("Provider base URL is required")
        tail = value.rsplit("/", 1)[-1]
        if tail.startswith("v") and tail[1:].isdigit():
            return value
        return f"{value}/v1"

    def _headers(self) -> dict[str, str]:
        headers = {"content-type": "application/json"}
        if self.api_key:
            headers["authorization"] = f"Bearer {self.api_key}"
        return headers

    def _client(self, timeout: float | None) -> httpx.AsyncClient:
        return httpx.AsyncClient(
            timeout=timeout,
            transport=self.transport,
            headers=self._headers(),
        )

    async def health(self) -> bool:
        try:
            async with self._client(5.0) as client:
                response = await client.get(f"{self.base_url}/models")
                response.raise_for_status()
                return True
        except httpx.HTTPError:
            return False

    async def list_models(self) -> list[ModelInfo]:
        payload = await self._get("/models")
        entries = payload.get("data")
        if not isinstance(entries, list):
            raise OpenAICompatUnavailable("Provider returned an invalid model list")
        models: list[ModelInfo] = []
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            name = str(entry.get("id") or "").strip()
            if not name:
                continue
            capabilities = infer_capabilities(f"{name} {entry.get('owned_by', '')}")
            models.append(
                ModelInfo(
                    name=name,
                    model_type="embedding" if capabilities == ["embedding"] else "language",
                    state=ModelState.INSTALLED,
                    capabilities=capabilities,
                    runtime=self.label,
                    modified_at=self._parse_epoch(entry.get("created")),
                )
            )
        models.sort(key=lambda model: model.name)
        return models

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        payload = self._chat_payload(request, stream=False)
        response = await self._post("/chat/completions", payload)
        choice = (response.get("choices") or [{}])[0]
        message = choice.get("message") or {}
        return self._ollama_message(
            request.model,
            str(message.get("content") or ""),
            done=True,
            finish_reason=choice.get("finish_reason"),
            usage=response.get("usage"),
        )

    async def chat_stream(self, request: ChatRequest) -> AsyncIterator[dict[str, Any]]:
        payload = self._chat_payload(request, stream=True)
        try:
            async with (
                self._client(None) as client,
                client.stream(
                    "POST",
                    f"{self.base_url}/chat/completions",
                    json=payload,
                ) as response,
            ):
                await self._raise_for_status(response)
                async for line in response.aiter_lines():
                    event = self._stream_event(line)
                    if event is None:
                        continue
                    if event is _DONE:
                        break
                    choice = (event.get("choices") or [{}])[0]
                    delta = choice.get("delta") or {}
                    content = str(delta.get("content") or "")
                    finish_reason = choice.get("finish_reason")
                    if content:
                        yield self._ollama_message(request.model, content, done=False)
                    if finish_reason:
                        break
        except httpx.HTTPError as exc:
            raise OpenAICompatUnavailable(str(exc)) from exc
        yield self._ollama_message(request.model, "", done=True)

    async def embed(self, model: str, inputs: list[str]) -> list[list[float]]:
        response = await self._post("/embeddings", {"model": model, "input": inputs})
        entries = response.get("data")
        if not isinstance(entries, list) or len(entries) != len(inputs):
            raise OpenAICompatUnavailable("Provider returned an invalid embedding response")
        ordered = sorted(entries, key=lambda entry: int(entry.get("index", 0)))
        vectors = [entry.get("embedding") for entry in ordered]
        if any(not isinstance(vector, list) for vector in vectors):
            raise OpenAICompatUnavailable("Provider returned an invalid embedding response")
        return [[float(value) for value in vector] for vector in vectors]

    async def extract_graph(self, model: str, content: str) -> dict[str, list[dict[str, str]]]:
        payload: dict[str, Any] = {
            "model": model,
            "stream": False,
            "temperature": 0,
            "messages": graph_messages(content),
            "response_format": {"type": "json_object"},
        }
        try:
            response = await self._post("/chat/completions", payload)
        except OpenAICompatUnavailable:
            # Not every OpenAI-compatible host implements structured output.
            payload.pop("response_format")
            response = await self._post("/chat/completions", payload)
        choice = (response.get("choices") or [{}])[0]
        parsed = decode_json_object((choice.get("message") or {}).get("content"))
        return normalize_graph_result(parsed)

    async def pull(self, name: str) -> AsyncIterator[dict[str, Any]]:
        raise ProviderReadOnly(f"{self.label} hosts its models remotely; downloads do not apply")
        yield {}  # pragma: no cover - keeps this an async generator

    async def unload(self, name: str) -> None:
        raise ProviderReadOnly(f"{self.label} hosts its models remotely; unloading does not apply")

    async def delete(self, name: str) -> None:
        raise ProviderReadOnly(f"{self.label} hosts its models remotely; deletion does not apply")

    def _chat_payload(self, request: ChatRequest, *, stream: bool) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "model": request.model,
            "messages": [message.model_dump() for message in request.messages],
            "stream": stream,
        }
        for key, value in request.options.items():
            alias = OPTION_ALIASES.get(key)
            if alias:
                payload[alias] = value
        return payload

    @staticmethod
    def _ollama_message(
        model: str,
        content: str,
        *,
        done: bool,
        finish_reason: str | None = None,
        usage: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        event: dict[str, Any] = {
            "model": model,
            "created_at": datetime.now(UTC).isoformat(),
            "message": {"role": "assistant", "content": content},
            "done": done,
        }
        if finish_reason:
            event["done_reason"] = finish_reason
        if usage:
            event["prompt_eval_count"] = usage.get("prompt_tokens", 0)
            event["eval_count"] = usage.get("completion_tokens", 0)
        return event

    @staticmethod
    def _stream_event(line: str) -> dict[str, Any] | object | None:
        value = line.strip()
        if not value or not value.startswith("data:"):
            return None
        data = value[5:].strip()
        if data == "[DONE]":
            return _DONE
        try:
            parsed = json.loads(data)
        except json.JSONDecodeError:
            return None
        return parsed if isinstance(parsed, dict) else None

    @staticmethod
    def _parse_epoch(value: Any) -> datetime | None:
        try:
            return datetime.fromtimestamp(float(value), tz=UTC) if value else None
        except (TypeError, ValueError, OSError):
            return None

    async def _get(self, path: str) -> dict[str, Any]:
        try:
            async with self._client(self.timeout) as client:
                response = await client.get(f"{self.base_url}{path}")
                await self._raise_for_status(response)
                return response.json()
        except httpx.HTTPError as exc:
            raise OpenAICompatUnavailable(str(exc)) from exc

    async def _post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        try:
            async with self._client(self.timeout) as client:
                response = await client.post(f"{self.base_url}{path}", json=payload)
                await self._raise_for_status(response)
                return response.json()
        except httpx.HTTPError as exc:
            raise OpenAICompatUnavailable(str(exc)) from exc

    @staticmethod
    async def _raise_for_status(response: httpx.Response) -> None:
        """Surface the provider's own error text, which usually names the real problem."""
        if not response.is_error:
            return
        detail = ""
        try:
            body = await response.aread()
            parsed = json.loads(body)
            if isinstance(parsed, dict):
                error = parsed.get("error")
                message = error.get("message") if isinstance(error, dict) else error
                detail = str(message or parsed.get("detail") or "")
        except (httpx.HTTPError, json.JSONDecodeError, ValueError):
            detail = ""
        raise OpenAICompatUnavailable(
            f"HTTP {response.status_code} from provider{f': {detail}' if detail else ''}"
        )
