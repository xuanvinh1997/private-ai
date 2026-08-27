from __future__ import annotations

import json
from typing import Any

import httpx
import pytest

from private_ai_api.schemas import ChatMessage, ChatRequest
from private_ai_api.services.gpu_lease import GpuLeaseManager, InsufficientVram
from private_ai_api.services.ollama import OllamaClient


def ollama_transport(state: dict[str, Any]) -> httpx.MockTransport:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/api/tags":
            return httpx.Response(
                200,
                json={
                    "models": [
                        {
                            "name": "tiny:latest",
                            "size": 60,
                            "modified_at": "2026-01-01T00:00:00Z",
                            "details": {"family": "qwen", "quantization_level": "Q4"},
                        },
                        {
                            "name": "embeddinggemma:latest",
                            "size": 10,
                            "modified_at": "2026-01-01T00:00:00Z",
                            "details": {"family": "embeddinggemma"},
                        },
                    ]
                },
            )
        if request.url.path == "/api/ps":
            models = []
            if state.get("loaded"):
                models.append({"name": "tiny:latest", "size_vram": 42})
            if state.get("embedding_loaded"):
                models.append({"name": "embeddinggemma:latest", "size_vram": 8})
            return httpx.Response(200, json={"models": models})
        if request.url.path == "/api/chat":
            payload = json.loads(request.content)
            state["chat_calls"] = state.get("chat_calls", 0) + 1
            state["loaded"] = True
            if "format" in payload:
                if state.get("empty_structured") and isinstance(payload["format"], dict):
                    return httpx.Response(
                        200,
                        json={"message": {"role": "assistant", "content": ""}},
                    )
                return httpx.Response(
                    200,
                    json={"message": {"role": "assistant", "content": json.dumps({
                        "entities": [
                            {"name": "OpenAI", "kind": "organization"},
                            {"name": "San Francisco", "kind": "place"},
                        ],
                        "relations": [
                            {
                                "source": "OpenAI",
                                "target": "San Francisco",
                                "relation": "based in",
                            }
                        ],
                    })}},
                )
            return httpx.Response(200, json={"message": {"role": "assistant", "content": "OK"}})
        if request.url.path == "/api/embed":
            payload = json.loads(request.content)
            state["embedding_loaded"] = True
            return httpx.Response(200, json={"embeddings": [[1.0, 0.0] for _ in payload["input"]]})
        if request.url.path == "/api/generate":
            state["loaded"] = False
            return httpx.Response(200, json={"done": True})
        return httpx.Response(404)

    return httpx.MockTransport(handler)


def chat_request() -> ChatRequest:
    return ChatRequest(
        model="tiny",
        messages=[ChatMessage(role="user", content="Reply OK")],
        stream=False,
    )


@pytest.mark.asyncio
async def test_ollama_chat_reserves_model_until_unload() -> None:
    state: dict[str, Any] = {}
    leases = GpuLeaseManager(capacity_bytes=100)
    client = OllamaClient(
        "http://ollama.test",
        gpu_leases=leases,
        transport=ollama_transport(state),
    )

    result = await client.chat(chat_request())

    assert result["message"]["content"] == "OK"
    assert leases.reserved_bytes == 66
    assert state["chat_calls"] == 1

    await client.unload("tiny")
    assert leases.reserved_bytes == 0


@pytest.mark.asyncio
async def test_ollama_embedding_uses_same_gpu_inventory() -> None:
    state: dict[str, Any] = {"loaded": True}
    leases = GpuLeaseManager(capacity_bytes=100)
    client = OllamaClient(
        "http://ollama.test",
        gpu_leases=leases,
        transport=ollama_transport(state),
    )

    vectors = await client.embed("embeddinggemma", ["one", "two"])

    assert vectors == [[1.0, 0.0], [1.0, 0.0]]
    assert leases.reserved_bytes == 53
    assert {lease["owner"] for lease in leases.snapshot()["leases"]} == {
        "ollama:tiny:latest",
        "ollama:embeddinggemma:latest",
    }


@pytest.mark.asyncio
async def test_ollama_does_not_start_model_when_capacity_is_insufficient() -> None:
    state: dict[str, Any] = {}
    leases = GpuLeaseManager(capacity_bytes=50)
    client = OllamaClient(
        "http://ollama.test",
        gpu_leases=leases,
        transport=ollama_transport(state),
    )

    with pytest.raises(InsufficientVram):
        await client.chat(chat_request())

    assert state.get("chat_calls", 0) == 0
    assert leases.reserved_bytes == 0


@pytest.mark.asyncio
async def test_health_keeps_pending_reservation_but_removes_expired_model() -> None:
    state: dict[str, Any] = {}
    leases = GpuLeaseManager(capacity_bytes=100)
    client = OllamaClient(
        "http://ollama.test",
        gpu_leases=leases,
        transport=ollama_transport(state),
    )
    await leases.reserve("ollama:tiny:latest", 66)

    assert await client.health() is True
    assert leases.reserved_bytes == 66

    await leases.mark_observed("ollama:tiny:latest")
    assert await client.health() is True
    assert leases.reserved_bytes == 0


@pytest.mark.asyncio
async def test_ollama_extracts_normalized_graph_facts() -> None:
    state: dict[str, Any] = {}
    client = OllamaClient(
        "http://ollama.test",
        transport=ollama_transport(state),
    )

    facts = await client.extract_graph(
        "tiny",
        "OpenAI is based in San Francisco.",
    )

    assert facts == {
        "entities": [
            {"key": "openai", "name": "OpenAI", "kind": "organization"},
            {"key": "san francisco", "name": "San Francisco", "kind": "place"},
        ],
        "relations": [
            {
                "source_key": "openai",
                "target_key": "san francisco",
                "relation": "based_in",
            }
        ],
    }


@pytest.mark.asyncio
async def test_ollama_graph_extraction_retries_empty_structured_output() -> None:
    state: dict[str, Any] = {"empty_structured": True}
    client = OllamaClient(
        "http://ollama.test",
        transport=ollama_transport(state),
    )

    facts = await client.extract_graph("tiny", "OpenAI is based in San Francisco.")

    assert facts["relations"][0]["relation"] == "based_in"
    assert state["chat_calls"] == 2
