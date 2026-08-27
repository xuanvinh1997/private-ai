from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from private_ai_api import config
from private_ai_api.config import Settings, detect_gpu_capacity_bytes
from private_ai_api.main import create_app


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
