from __future__ import annotations

import json

import httpx
import pytest
from fastapi.testclient import TestClient

from private_ai_api.schemas import ChatRequest
from private_ai_api.services.openai_compat import OpenAICompatClient
from private_ai_api.services.provider import ProviderReadOnly
from private_ai_api.services.provider_registry import LOCAL_PROVIDER_ID

REMOTE_MODELS = {
    "data": [
        {"id": "gpt-4o-mini", "object": "model", "created": 1_700_000_000},
        {"id": "text-embedding-3-small", "object": "model"},
    ]
}


def _remote_transport() -> httpx.MockTransport:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path.endswith("/models"):
            return httpx.Response(200, json=REMOTE_MODELS)
        if request.url.path.endswith("/chat/completions"):
            assert request.headers["authorization"] == "Bearer secret-key"
            return httpx.Response(
                200,
                json={
                    "choices": [
                        {
                            "message": {"role": "assistant", "content": "xin chào"},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 7, "completion_tokens": 3},
                },
            )
        return httpx.Response(404, json={"error": {"message": "unknown route"}})

    return httpx.MockTransport(handler)


def _register_remote(client: TestClient) -> str:
    client.app.state.services.providers.transport = _remote_transport()
    created = client.post(
        "/api/v1/providers",
        json={
            "name": "Bộ định tuyến OpenAI",
            "kind": "openai",
            "base_url": "https://host.example/v1",
            "api_key": "secret-key",
        },
    )
    assert created.status_code == 201
    provider_id = created.json()["id"]
    assert client.post(f"/api/v1/providers/{provider_id}/activate").status_code == 200
    return provider_id


def test_local_provider_is_the_default_selection(client: TestClient) -> None:
    providers = client.get("/api/v1/providers")
    assert providers.status_code == 200
    body = providers.json()
    assert len(body) == 1
    assert body[0]["id"] == LOCAL_PROVIDER_ID
    assert body[0]["builtin"] is True
    assert body[0]["active"] is True


def test_api_key_is_stored_but_never_returned(client: TestClient) -> None:
    provider_id = _register_remote(client)
    record = next(
        item for item in client.get("/api/v1/providers").json() if item["id"] == provider_id
    )
    assert record["has_api_key"] is True
    assert record["active"] is True
    assert "api_key" not in record
    assert client.get("/api/v1/health").json()["provider"]["id"] == provider_id


def test_active_provider_serves_models_and_chat(client: TestClient) -> None:
    _register_remote(client)

    inventory = {model["name"]: model for model in client.get("/api/v1/models").json()}
    assert inventory["gpt-4o-mini"]["runtime"] == "Bộ định tuyến OpenAI"
    assert inventory["gpt-4o-mini"]["model_type"] == "language"
    assert inventory["text-embedding-3-small"]["model_type"] == "embedding"

    answer = client.post(
        "/api/v1/chat",
        json={"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "chào"}]},
    )
    assert answer.status_code == 200
    assert answer.json()["message"]["content"] == "xin chào"


def test_remote_models_reject_local_lifecycle_actions(client: TestClient) -> None:
    _register_remote(client)
    unloaded = client.post("/api/v1/models/gpt-4o-mini/unload")
    assert unloaded.status_code == 422
    assert "remotely" in unloaded.json()["detail"]


def test_probe_reports_an_unreachable_host(client: TestClient) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("refused", request=request)

    client.app.state.services.providers.transport = httpx.MockTransport(handler)
    probe = client.post(
        "/api/v1/providers/probe",
        json={"kind": "openai", "base_url": "https://host.example", "api_key": ""},
    )
    assert probe.status_code == 200
    assert probe.json()["reachable"] is False


def test_probe_lists_models_for_a_saved_provider(client: TestClient) -> None:
    provider_id = _register_remote(client)
    probe = client.post(f"/api/v1/providers/{provider_id}/probe")
    assert probe.status_code == 200
    body = probe.json()
    assert body["reachable"] is True
    assert body["model_count"] == 2
    assert "gpt-4o-mini" in body["models"]


def test_deleting_the_active_provider_falls_back_to_local(client: TestClient) -> None:
    provider_id = _register_remote(client)
    removed = client.delete(f"/api/v1/providers/{provider_id}?confirmed=true")
    assert removed.status_code == 204
    assert client.get("/api/v1/health").json()["provider"]["id"] == LOCAL_PROVIDER_ID


def test_the_local_provider_can_be_removed_once_another_one_exists(client: TestClient) -> None:
    provider_id = _register_remote(client)
    removed = client.delete(f"/api/v1/providers/{LOCAL_PROVIDER_ID}?confirmed=true")
    assert removed.status_code == 204
    assert [item["id"] for item in client.get("/api/v1/providers").json()] == [provider_id]
    assert client.get("/api/v1/health").json()["provider"]["id"] == provider_id


def test_a_removed_local_provider_is_not_seeded_again(client: TestClient, tmp_path) -> None:
    from private_ai_api.config import Settings
    from private_ai_api.main import create_app

    _register_remote(client)
    assert client.delete(f"/api/v1/providers/{LOCAL_PROVIDER_ID}?confirmed=true").status_code == 204

    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as restarted:
        listed = [item["id"] for item in restarted.get("/api/v1/providers").json()]
        assert LOCAL_PROVIDER_ID not in listed


def test_removing_every_provider_leaves_a_usable_but_unconfigured_app(client: TestClient) -> None:
    provider_id = _register_remote(client)
    for target in (provider_id, LOCAL_PROVIDER_ID):
        assert client.delete(f"/api/v1/providers/{target}?confirmed=true").status_code == 204

    assert client.get("/api/v1/providers").json() == []
    health = client.get("/api/v1/health").json()
    assert health["provider"] is None
    assert health["services"]["provider"] == "not_configured"

    # The inventory still answers, so the model page renders instead of erroring out.
    assert client.get("/api/v1/models").status_code == 200
    refused = client.post(
        "/api/v1/chat",
        json={"model": "gpt-4o-mini", "messages": [{"role": "user", "content": "chào"}]},
    )
    assert refused.status_code == 503
    assert refused.json()["detail"] == "No AI provider is configured"


def test_builtin_provider_can_be_pointed_at_another_host(client: TestClient) -> None:
    """The desktop shell runs beside an Ollama that often lives in WSL2 or on the LAN."""
    moved = client.patch(
        f"/api/v1/providers/{LOCAL_PROVIDER_ID}",
        json={"name": "Ollama trong WSL2", "base_url": "http://172.20.0.2:11434"},
    )
    assert moved.status_code == 200
    assert moved.json()["base_url"] == "http://172.20.0.2:11434"
    assert moved.json()["builtin"] is True

    services = client.app.state.services
    assert services.ollama.base_url == "http://172.20.0.2:11434"
    assert services.providers.active_client() is services.ollama
    assert client.get("/api/v1/health").json()["provider"]["name"] == "Ollama trong WSL2"

    listed = client.get("/api/v1/providers").json()
    assert [item["id"] for item in listed] == [LOCAL_PROVIDER_ID]


def test_a_moved_local_host_survives_a_restart(client: TestClient, tmp_path) -> None:
    from private_ai_api.config import Settings
    from private_ai_api.main import create_app

    client.patch(
        f"/api/v1/providers/{LOCAL_PROVIDER_ID}",
        json={"base_url": "http://172.20.0.2:11434"},
    )
    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as restarted:
        assert restarted.app.state.services.ollama.base_url == "http://172.20.0.2:11434"


def test_base_url_must_be_absolute(client: TestClient) -> None:
    rejected = client.post(
        "/api/v1/providers",
        json={"name": "Sai", "kind": "openai", "base_url": "host.example"},
    )
    assert rejected.status_code == 422


@pytest.mark.asyncio
async def test_streaming_translates_openai_chunks_to_ollama_events() -> None:
    chunks = [
        {"choices": [{"delta": {"content": "xin "}}]},
        {"choices": [{"delta": {"content": "chào"}}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    ]
    body = "".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n"

    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})

    provider = OpenAICompatClient(
        "https://host.example",
        "key",
        transport=httpx.MockTransport(handler),
    )
    request = ChatRequest(
        model="gpt-4o-mini",
        messages=[{"role": "user", "content": "chào"}],
        stream=True,
    )
    events = [event async for event in provider.chat_stream(request)]
    assert "".join(event["message"]["content"] for event in events) == "xin chào"
    assert events[-1]["done"] is True


@pytest.mark.asyncio
async def test_remote_provider_refuses_model_downloads() -> None:
    provider = OpenAICompatClient("https://host.example", "key")
    with pytest.raises(ProviderReadOnly):
        async for _ in provider.pull("gpt-4o-mini"):
            pass


def test_embedding_default_survives_a_restart(client: TestClient, tmp_path) -> None:
    from private_ai_api.config import Settings
    from private_ai_api.main import create_app

    _register_remote(client)
    selected = client.put(
        "/api/v1/models/defaults/embedding",
        json={"model": "text-embedding-3-small"},
    )
    assert selected.status_code == 200
    assert client.app.state.services.lightrag.embedding_model == "text-embedding-3-small"

    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as restarted:
        services = restarted.app.state.services
        assert services.lightrag.embedding_model == "text-embedding-3-small"
        assert services.memory_service.embedding_model == "text-embedding-3-small"


def test_deleting_the_active_provider_clears_the_stored_selection(client: TestClient) -> None:
    provider_id = _register_remote(client)
    client.delete(f"/api/v1/providers/{provider_id}?confirmed=true")
    stored = client.app.state.services.database.fetch_one(
        "SELECT value FROM app_state WHERE key = 'active_provider_id'"
    )
    assert stored is not None
    assert stored["value"] == LOCAL_PROVIDER_ID
