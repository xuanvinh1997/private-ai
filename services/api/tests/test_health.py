from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from private_ai_api import config
from private_ai_api.config import Settings, detect_gpu_capacity_bytes
from private_ai_api.main import create_app
from private_ai_api.services.provider import runs_on_device


def test_health_reports_gateway_and_database(client: TestClient) -> None:
    response = client.get("/api/v1/health")

    assert response.status_code == 200
    payload = response.json()
    assert payload["status"] == "ok"
    assert payload["services"]["api"] == "online"
    assert payload["services"]["database"] == "online"

    gpu = payload["gpu"]
    assert gpu["capacity_bytes"] > 0
    assert isinstance(gpu["unified_memory"], bool)
    if gpu["unified_memory"]:
        # No separate VRAM: the budget has to fit inside system RAM.
        assert gpu["total_memory_bytes"] > 0
        assert gpu["capacity_bytes"] <= gpu["total_memory_bytes"]


def test_gpu_capacity_can_be_overridden(tmp_path: Path) -> None:
    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
        gpu_capacity_bytes=8 * 1024**3,
    )
    with TestClient(create_app(settings)) as client:
        assert client.get("/api/v1/health").json()["gpu"]["capacity_bytes"] == 8 * 1024**3


@pytest.mark.parametrize(
    ("total_gib", "wired_limit_mb", "expected_gib"),
    [
        (48, 0, 36.0),  # macOS default share on a large-memory SoC
        (16, 0, 16 * 2 / 3),  # smaller machines get a smaller share
        (48, 40 * 1024, 40.0),  # an explicit iogpu.wired_limit_mb wins
    ],
)
def test_unified_memory_budget_tracks_the_machine(
    monkeypatch: pytest.MonkeyPatch,
    total_gib: int,
    wired_limit_mb: int,
    expected_gib: float,
) -> None:
    monkeypatch.setattr(config, "is_unified_memory", lambda: True)
    monkeypatch.setattr(config, "total_memory_bytes", lambda: total_gib * 1024**3)
    monkeypatch.setattr(config, "_sysctl_int", lambda name: wired_limit_mb)

    assert detect_gpu_capacity_bytes() == pytest.approx(expected_gib * 1024**3)


def test_non_unified_memory_keeps_the_documented_default(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(config, "is_unified_memory", lambda: False)

    assert detect_gpu_capacity_bytes() == config.FALLBACK_GPU_CAPACITY_BYTES


def _remote_transport() -> httpx.MockTransport:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path.endswith("/models"):
            return httpx.Response(200, json={"data": [{"id": "gpt-4o-mini", "object": "model"}]})
        return httpx.Response(404, json={"error": {"message": "unknown route"}})

    return httpx.MockTransport(handler)


def test_health_reports_the_active_provider_rather_than_a_fixed_runtime(
    client: TestClient,
) -> None:
    services = client.get("/api/v1/health").json()["services"]

    # Ollama is one provider among others, so it no longer stands for the AI service itself.
    assert "ollama" not in services
    assert services["provider"] in {"online", "offline"}
    assert services["local_runtime"] in {"online", "offline"}


def test_health_marks_a_loopback_provider_as_on_device(client: TestClient) -> None:
    payload = client.get("/api/v1/health").json()

    assert payload["provider"]["base_url"].startswith("http://127.0.0.1")
    assert payload["provider"]["on_device"] is True


def test_health_marks_a_remote_provider_as_off_device(client: TestClient) -> None:
    client.app.state.services.providers.transport = _remote_transport()
    created = client.post(
        "/api/v1/providers",
        json={
            "name": "Bộ định tuyến từ xa",
            "kind": "openai",
            "base_url": "https://host.example/v1",
            "api_key": "secret-key",
        },
    )
    assert created.status_code == 201
    assert client.post(f"/api/v1/providers/{created.json()['id']}/activate").status_code == 200

    payload = client.get("/api/v1/health").json()

    assert payload["provider"]["on_device"] is False
    # The local server keeps its own row: it still backs the GPU and model panels.
    assert "local_runtime" in payload["services"]


@pytest.mark.parametrize(
    ("base_url", "expected"),
    [
        ("http://127.0.0.1:11434", True),
        ("http://localhost:11434/v1", True),
        ("http://[::1]:11434", True),
        ("https://api.openai.com/v1", False),
        # A local record repointed at WSL2 or another machine is no longer on-device.
        ("http://192.168.1.20:11434", False),
        ("", False),
    ],
)
def test_runs_on_device_only_trusts_the_loopback_interface(base_url: str, expected: bool) -> None:
    assert runs_on_device(base_url) is expected
