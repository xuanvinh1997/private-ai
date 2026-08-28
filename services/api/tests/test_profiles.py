from __future__ import annotations

from fastapi.testclient import TestClient


def test_a_fresh_database_has_one_unnamed_profile(client: TestClient) -> None:
    """An empty name is how the web app knows to run onboarding."""
    profiles = client.get("/api/v1/profiles").json()
    assert len(profiles) == 1
    assert profiles[0]["display_name"] == ""
    assert profiles[0]["active"] is True
    assert client.get("/api/v1/profiles/active").json()["id"] == profiles[0]["id"]


def test_onboarding_names_the_profile_that_is_already_there(client: TestClient) -> None:
    profile_id = client.get("/api/v1/profiles/active").json()["id"]

    named = client.patch(f"/api/v1/profiles/{profile_id}", json={"display_name": " Vinh "})
    assert named.status_code == 200
    assert named.json()["display_name"] == "Vinh"
    assert client.get("/api/v1/profiles/active").json()["display_name"] == "Vinh"
    # Naming does not add a second profile.
    assert len(client.get("/api/v1/profiles").json()) == 1


def test_adding_a_profile_switches_to_it(client: TestClient) -> None:
    first = client.get("/api/v1/profiles/active").json()["id"]

    created = client.post("/api/v1/profiles", json={"display_name": "Khách"})
    assert created.status_code == 201
    assert created.json()["active"] is True
    assert client.get("/api/v1/profiles/active").json()["id"] == created.json()["id"]

    switched_back = client.post(f"/api/v1/profiles/{first}/activate")
    assert switched_back.status_code == 200
    assert client.get("/api/v1/profiles/active").json()["id"] == first


def test_memories_follow_the_active_profile(client: TestClient) -> None:
    client.post("/api/v1/memory", json={"type": "fact", "content": "Tôi ở Hà Nội"})
    assert len(client.get("/api/v1/memory").json()) == 1

    guest = client.post("/api/v1/profiles", json={"display_name": "Khách"}).json()
    # The new profile starts with nothing of its own.
    assert client.get("/api/v1/memory").json() == []
    client.post("/api/v1/memory", json={"type": "fact", "content": "Tôi thích trà"})

    guest_memories = client.get("/api/v1/memory").json()
    assert [memory["content"] for memory in guest_memories] == ["Tôi thích trà"]
    assert guest_memories[0]["user_id"] == guest["id"]

    owner = next(p for p in client.get("/api/v1/profiles").json() if p["id"] != guest["id"])
    client.post(f"/api/v1/profiles/{owner['id']}/activate")
    assert [m["content"] for m in client.get("/api/v1/memory").json()] == ["Tôi ở Hà Nội"]


def test_deleting_a_profile_takes_its_memories_and_hands_over_the_session(
    client: TestClient,
) -> None:
    owner = client.get("/api/v1/profiles/active").json()["id"]
    guest = client.post("/api/v1/profiles", json={"display_name": "Khách"}).json()["id"]
    client.post("/api/v1/memory", json={"type": "fact", "content": "Ghi chú của khách"})

    assert client.delete(f"/api/v1/profiles/{guest}").status_code == 422
    assert client.delete(f"/api/v1/profiles/{guest}?confirmed=false").status_code == 409

    removed = client.delete(f"/api/v1/profiles/{guest}?confirmed=true")
    assert removed.status_code == 204
    assert client.get("/api/v1/profiles/active").json()["id"] == owner
    assert client.get("/api/v1/memory", params={"user_id": guest}).json() == []


def test_the_last_profile_cannot_be_deleted(client: TestClient) -> None:
    only = client.get("/api/v1/profiles/active").json()["id"]
    refused = client.delete(f"/api/v1/profiles/{only}?confirmed=true")
    assert refused.status_code == 409
    assert len(client.get("/api/v1/profiles").json()) == 1
