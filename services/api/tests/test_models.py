from __future__ import annotations

from fastapi.testclient import TestClient

from private_ai_api.schemas import ModelInfo, ModelState
from private_ai_api.services.asr import ASR_MODEL_NAME


def test_model_inventory_defaults_and_asr_lifecycle(
    client: TestClient,
    monkeypatch,
) -> None:
    async def fake_list_models() -> list[ModelInfo]:
        return [
            ModelInfo(
                name="chat-model",
                model_type="language",
                state=ModelState.UNLOADED,
                size_bytes=42,
                capabilities=["completion"],
            )
        ]

    close_calls = 0

    async def fake_close() -> None:
        nonlocal close_calls
        close_calls += 1

    monkeypatch.setattr(client.app.state.services.ollama, "list_models", fake_list_models)
    monkeypatch.setattr(client.app.state.services.asr, "close", fake_close)

    inventory = client.get("/api/v1/models")
    assert inventory.status_code == 200
    models = {model["name"]: model for model in inventory.json()}
    assert models["chat-model"]["runtime"] == "ollama"
    assert models["chat-model"]["default_for"] == ["chat"]
    assert models[ASR_MODEL_NAME]["model_type"] == "asr"
    assert models[ASR_MODEL_NAME]["default_for"] == ["asr"]

    selected = client.put(
        "/api/v1/models/defaults/chat",
        json={"model": "chat-model"},
    )
    assert selected.status_code == 200
    assert selected.json() == {"task": "chat", "model": "chat-model"}

    unloaded = client.post(f"/api/v1/models/{ASR_MODEL_NAME}/unload")
    assert unloaded.status_code == 204
    assert close_calls == 1
    events = client.get("/api/v1/models/events").json()
    assert events[0]["model_name"] == ASR_MODEL_NAME
    assert events[0]["action"] == "unload"
    assert events[0]["status"] == "completed"


def test_default_model_rejects_wrong_runtime_type(client: TestClient, monkeypatch) -> None:
    async def fake_list_models() -> list[ModelInfo]:
        return [
            ModelInfo(
                name="embedding-only",
                model_type="embedding",
                capabilities=["embedding"],
            )
        ]

    monkeypatch.setattr(client.app.state.services.ollama, "list_models", fake_list_models)
    response = client.put(
        "/api/v1/models/defaults/chat",
        json={"model": "embedding-only"},
    )
    assert response.status_code == 422


def test_default_vision_model_requires_vision_capability(client: TestClient, monkeypatch) -> None:
    async def fake_list_models() -> list[ModelInfo]:
        return [
            ModelInfo(
                name="text-only",
                model_type="language",
                capabilities=["chat"],
            ),
            ModelInfo(
                name="qwen3-vl:8b",
                model_type="language",
                capabilities=["chat", "vision"],
            ),
        ]

    monkeypatch.setattr(client.app.state.services.ollama, "list_models", fake_list_models)
    rejected = client.put(
        "/api/v1/models/defaults/vision",
        json={"model": "text-only"},
    )
    selected = client.put(
        "/api/v1/models/defaults/vision",
        json={"model": "qwen3-vl:8b"},
    )

    assert rejected.status_code == 422
    assert selected.status_code == 200
    assert selected.json() == {"task": "vision", "model": "qwen3-vl:8b"}
