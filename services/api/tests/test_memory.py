from fastapi.testclient import TestClient


def test_memory_lifecycle_requires_confirmation(client: TestClient) -> None:
    created = client.post(
        "/api/v1/memory",
        json={
            "type": "preference",
            "content": "Trả lời bằng tiếng Việt",
            "source": "user",
            "confidence": 1,
        },
    )
    assert created.status_code == 201
    memory_id = created.json()["id"]

    listed = client.get("/api/v1/memory").json()
    assert [item["id"] for item in listed] == [memory_id]
    searched = client.get("/api/v1/memory/search", params={"q": "tiếng Việt"})
    assert searched.status_code == 200
    assert searched.json()[0]["id"] == memory_id

    disabled = client.post(f"/api/v1/memory/{memory_id}/disable")
    assert disabled.status_code == 200
    assert disabled.json()["enabled"] is False
    assert client.get("/api/v1/memory").json() == []

    enabled = client.post(f"/api/v1/memory/{memory_id}/enable")
    assert enabled.status_code == 200
    assert enabled.json()["enabled"] is True
    updated = client.patch(
        f"/api/v1/memory/{memory_id}",
        json={
            "type": "preference",
            "content": "Trả lời ngắn gọn bằng tiếng Việt",
            "source": "user",
            "confidence": 1,
        },
    )
    assert updated.status_code == 200
    assert updated.json()["content"] == "Trả lời ngắn gọn bằng tiếng Việt"

    refused = client.delete(f"/api/v1/memory/{memory_id}?confirmed=false")
    assert refused.status_code == 409
    forgotten = client.delete(f"/api/v1/memory/{memory_id}?confirmed=true")
    assert forgotten.status_code == 204
