from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from private_ai_api.config import Settings
from private_ai_api.main import create_app


@pytest.fixture
def client(tmp_path: Path) -> Iterator[TestClient]:
    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as test_client:
        yield test_client
