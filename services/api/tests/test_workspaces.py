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
    client.app.state.services.ai = fake_ollama

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
    client.app.state.services.ai = fake_ollama
    uploaded = client.post(
        "/api/v1/documents",
        files={
            "file": (
                "private-notes.md",
                b"The internal launch codename is Starfruit-Delta.",
                "text/markdown",
            )
        },
        data={"workspace_id": "research"},
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
    assert client.app.state.services.lightrag.last_search_mode == "naive"
    assert fake_ollama.last_request.messages[0].role == "system"
    assert "Starfruit-Delta" in fake_ollama.last_request.messages[0].content
    assert "private-notes.md" in fake_ollama.last_request.messages[0].content
    assert any(
        "Prefer concise answers" in message.content
        for message in fake_ollama.last_request.messages
        if message.role == "system"
    )

    graph_response = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={
            "model": "test-model",
            "content": "Use relationships for the launch codename",
            "rag_mode": "graph",
        },
    )
    assert graph_response.status_code == 200
    assert client.app.state.services.lightrag.last_search_mode == "mix"


def test_conversation_streams_and_persists_assistant_message(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ai = fake_ollama
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


class FakeToolCallingProvider:
    """Asks for a tool on the first turn, answers with what it learned on the second."""

    def __init__(self) -> None:
        self.rounds: list[ChatRequest] = []

    async def chat_stream(self, request: ChatRequest):
        self.rounds.append(request)
        if len(self.rounds) == 1:
            yield {
                "message": {"role": "assistant", "content": "Tôi sẽ tra cứu."},
                "done": False,
            }
            yield {
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {"id": "call_1", "function": {"name": "workspaces__list", "arguments": {}}}
                    ],
                },
                "done": True,
            }
            return
        yield {"message": {"role": "assistant", "content": " Đã tra cứu xong."}, "done": True}


def test_streaming_chat_continues_after_a_tool_call(client: TestClient) -> None:
    provider = FakeToolCallingProvider()
    client.app.state.services.ai = provider
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()

    with client.stream(
        "POST",
        f"/api/v1/conversations/{conversation['id']}/chat/stream",
        json={"model": "test-model", "content": "Có workspace nào?"},
    ) as response:
        body = "\n".join(response.iter_lines())

    assert response.status_code == 200
    assert '"type":"tool","name":"workspaces.list"' in body
    assert '"type":"done"' in body
    detail = client.get(f"/api/v1/conversations/{conversation['id']}").json()
    assert detail["messages"][-1]["content"] == "Tôi sẽ tra cứu. Đã tra cứu xong."
    # The second round sees its own preamble and the tool output, not just the question.
    transcript = provider.rounds[1].messages
    assert transcript[-2].content == "Tôi sẽ tra cứu."
    assert transcript[-1].role == "tool"


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


def test_documents_never_leak_between_workspaces(client: TestClient) -> None:
    fake_ollama = FakeOllama()
    client.app.state.services.ai = fake_ollama
    uploaded = client.post(
        "/api/v1/documents",
        files={
            "file": (
                "research-only.md",
                b"The internal launch codename is Starfruit-Delta.",
                "text/markdown",
            )
        },
        data={"workspace_id": "research"},
    )
    assert uploaded.status_code == 201

    # The library and the search endpoint are both scoped.
    listed = client.get("/api/v1/documents", params={"workspace_id": "personal"}).json()
    assert listed["items"] == []
    assert (
        client.get(
            "/api/v1/documents/search",
            params={"q": "Starfruit-Delta", "workspace_id": "personal"},
        ).json()
        == []
    )

    # A chat in another workspace must not be grounded on that document.
    conversation = client.post(
        "/api/v1/workspaces/personal/conversations",
        json={"model": "test-model"},
    ).json()
    response = client.post(
        f"/api/v1/conversations/{conversation['id']}/chat",
        json={"model": "test-model", "content": "What is the launch codename?"},
    )
    assert response.status_code == 200
    assert fake_ollama.last_request is not None
    assert not any(
        "Starfruit-Delta" in message.content for message in fake_ollama.last_request.messages
    )


def test_deleting_a_workspace_removes_its_documents_and_files(client: TestClient) -> None:
    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("doomed.md", b"# Doomed\ncontent", "text/markdown")},
        data={"workspace_id": "research"},
    ).json()
    source_path = Path(uploaded["source_path"])
    assert source_path.exists()

    assert client.delete("/api/v1/workspaces/research?confirmed=true").status_code == 204
    assert client.get(f"/api/v1/documents/{uploaded['id']}").status_code == 404
    assert not source_path.exists()
    assert client.get("/api/v1/documents", params={"workspace_id": "research"}).status_code == 404
