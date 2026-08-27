from __future__ import annotations

from pathlib import Path
from typing import Any

from fastapi.testclient import TestClient

from private_ai_api.config import Settings
from private_ai_api.main import create_app
from private_ai_api.schemas import ChatRequest


class FakeOllama:
    last_request: ChatRequest | None = None

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        self.last_request = request
        return {
            "message": {
                "role": "assistant",
                "content": f"Đã nhận: {request.messages[-1].content}",
            }
        }

    async def chat_stream(self, request: ChatRequest):
        self.last_request = request
        yield {"message": {"role": "assistant", "content": "Đã nhận: "}, "done": False}
        yield {
            "message": {"role": "assistant", "content": request.messages[-1].content},
            "done": False,
        }
        yield {"message": {"role": "assistant", "content": ""}, "done": True}


def test_workspace_conversation_and_persisted_chat(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ollama = fake_ollama

    seeded = client.get("/api/v1/workspaces")
    assert seeded.status_code == 200
    assert {item["id"] for item in seeded.json()} >= {"personal", "research", "private-ai"}

    workspace_response = client.post(
        "/api/v1/workspaces",
        json={"name": "Kiểm thử", "description": "Workspace tạm"},
    )
    assert workspace_response.status_code == 201
    workspace_id = workspace_response.json()["id"]

    conversation_response = client.post(
        f"/api/v1/workspaces/{workspace_id}/conversations",
        json={"model": "test-model"},
    )
    assert conversation_response.status_code == 201
    conversation_id = conversation_response.json()["id"]

    chat_response = client.post(
        f"/api/v1/conversations/{conversation_id}/chat",
        json={"model": "test-model", "content": "Xin chào"},
    )
    assert chat_response.status_code == 200
    detail = chat_response.json()
    assert detail["title"] == "Xin chào"
    assert [message["role"] for message in detail["messages"]] == ["user", "assistant"]
    assert detail["messages"][1]["content"] == "Đã nhận: Xin chào"

    reloaded = client.get(f"/api/v1/conversations/{conversation_id}")
    assert reloaded.status_code == 200
    assert reloaded.json()["messages"] == detail["messages"]

    listed = client.get(f"/api/v1/workspaces/{workspace_id}/conversations")
    assert listed.status_code == 200
    assert listed.json()[0]["message_count"] == 2

    refused = client.delete(f"/api/v1/workspaces/{workspace_id}?confirmed=false")
    assert refused.status_code == 409
    deleted = client.delete(f"/api/v1/workspaces/{workspace_id}?confirmed=true")
    assert deleted.status_code == 204
    assert client.get(f"/api/v1/conversations/{conversation_id}").status_code == 404


def test_conversation_injects_relevant_local_document(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ollama = fake_ollama
    uploaded = client.post(
        "/api/v1/documents",
        files={
            "file": (
                "private-notes.md",
                b"The internal launch codename is Starfruit-Delta.",
                "text/markdown",
            )
        },
    )
    assert uploaded.status_code == 201
    remembered = client.post(
        "/api/v1/memory",
        json={
            "type": "preference",
            "content": "Prefer concise answers.",
            "source": "user",
            "confidence": 1,
        },
    )
    assert remembered.status_code == 201
    conversation = client.post(
        "/api/v1/workspaces/research/conversations",
        json={"model": "test-model"},
    ).json()
    response = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={"model": "test-model", "content": "What is the launch codename?"},
    )
    assert response.status_code == 200
    assert fake_ollama.last_request is not None
    assert fake_ollama.last_request.messages[0].role == "system"
    assert "Starfruit-Delta" in fake_ollama.last_request.messages[0].content
    assert "private-notes.md" in fake_ollama.last_request.messages[0].content
    assert any(
        "Prefer concise answers" in message.content
        for message in fake_ollama.last_request.messages
        if message.role == "system"
    )


def test_conversation_streams_and_persists_assistant_message(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ollama = fake_ollama
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()

    with client.stream(
        "POST",
        f"/api/v1/conversations/{conversation['id']}/chat/stream",
        json={"model": "test-model", "content": "Xin chào streaming"},
    ) as response:
        body = "\n".join(response.iter_lines())

    assert response.status_code == 200
    assert '"type":"delta"' in body
    assert '"type":"done"' in body
    detail = client.get(f"/api/v1/conversations/{conversation['id']}").json()
    assert [message["role"] for message in detail["messages"]] == ["user", "assistant"]
    assert detail["messages"][-1]["content"] == "Đã nhận: Xin chào streaming"


def test_deleted_seed_workspace_stays_deleted_after_restart(tmp_path: Path) -> None:
    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as client:
        assert client.delete("/api/v1/workspaces/personal?confirmed=true").status_code == 204
        assert {item["id"] for item in client.get("/api/v1/workspaces").json()} == {
            "research",
            "private-ai",
        }

    with TestClient(create_app(settings)) as restarted:
        assert {item["id"] for item in restarted.get("/api/v1/workspaces").json()} == {
            "research",
            "private-ai",
        }
