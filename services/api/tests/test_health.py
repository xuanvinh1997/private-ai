from fastapi.testclient import TestClient


def test_health_reports_gateway_and_database(client: TestClient) -> None:
    response = client.get("/api/v1/health")

    assert response.status_code == 200
    payload = response.json()
    assert payload["status"] == "ok"
    assert payload["services"]["api"] == "online"
    assert payload["services"]["database"] == "online"
    assert payload["gpu"]["capacity_bytes"] == 96 * 1024**3

