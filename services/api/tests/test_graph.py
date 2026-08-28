from __future__ import annotations

from fastapi.testclient import TestClient


def _workspace(client: TestClient) -> str:
    response = client.post("/api/v1/workspaces", json={"name": "Nghiên cứu", "description": ""})
    assert response.status_code == 201
    return response.json()["id"]


def _upload(client: TestClient, workspace_id: str, text: str) -> None:
    response = client.post(
        "/api/v1/documents",
        data={"workspace_id": workspace_id},
        files={"file": ("ghi-chu.md", text.encode("utf-8"), "text/markdown")},
    )
    assert response.status_code == 201


def test_graph_returns_nodes_and_edges_of_one_workspace(client: TestClient) -> None:
    workspace_id = _workspace(client)
    _upload(client, workspace_id, "Ollama phục vụ mô hình cho Private AI trong WSL2.")

    response = client.get("/api/v1/graph", params={"workspace_id": workspace_id})

    assert response.status_code == 200
    payload = response.json()
    assert payload["entity"] == "*"
    assert payload["nodes"], "the workspace has indexed text, so the graph is not empty"
    assert payload["truncated"] is False
    ids = {node["id"] for node in payload["nodes"]}
    for edge in payload["edges"]:
        assert edge["source"] in ids and edge["target"] in ids


def test_graph_entities_filter_by_query(client: TestClient) -> None:
    workspace_id = _workspace(client)
    _upload(client, workspace_id, "Ollama phục vụ mô hình cho Private AI trong WSL2.")

    response = client.get(
        "/api/v1/graph/entities",
        params={"workspace_id": workspace_id, "q": "ollama"},
    )

    assert response.status_code == 200
    names = [item["name"] for item in response.json()]
    assert names == ["Ollama"]


def test_graph_rejects_an_unknown_workspace(client: TestClient) -> None:
    response = client.get("/api/v1/graph", params={"workspace_id": "missing"})

    assert response.status_code == 404
