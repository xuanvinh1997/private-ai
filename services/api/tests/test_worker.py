from __future__ import annotations

import asyncio
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from private_ai_api import worker
from private_ai_api.config import Settings

workspace = {"workspace_id": "personal"}


def test_the_api_only_queues_when_a_worker_owns_ingestion(
    queue_only_client: TestClient,
) -> None:
    """Nothing may be read in the API process: that is what froze every other request."""
    services = queue_only_client.app.state.services
    calls: list[str] = []

    async def refuse(document_id: str) -> None:
        calls.append(document_id)

    services.document_processor.process = refuse
    services.document_processor.index_document = refuse

    response = queue_only_client.post(
        "/api/v1/documents",
        files={"file": ("scan.pdf", b"%PDF-1.4 not really", "application/pdf")},
        data=workspace,
    )

    assert response.status_code == 201
    assert response.json()["status"] == "queued"
    assert calls == []
    # The row itself is the queue, so the worker has everything it needs to pick this up.
    assert worker._pending_count(services) == 1


def test_inline_mode_still_reads_the_file_itself(client: TestClient) -> None:
    """A bare `uvicorn private_ai_api.main:app` has no worker to hand the work to."""
    response = client.post(
        "/api/v1/documents",
        files={"file": ("notes.md", b"# Ghi chu\nnoi dung", "text/markdown")},
        data=workspace,
    )

    assert response.status_code == 201
    assert response.json()["status"] == "ready"
    assert worker._pending_count(client.app.state.services) == 0


@pytest.mark.asyncio
async def test_the_worker_drains_what_the_api_queued(
    queue_only_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    services = queue_only_client.app.state.services
    queue_only_client.post(
        "/api/v1/documents",
        files={"file": ("ghi-chu.txt", b"mot hai ba bon nam", "text/plain")},
        data=workspace,
    )
    assert worker._pending_count(services) == 1

    # The worker builds its own services from the same settings; here it gets the ones the
    # test client already wired to the in-memory index.
    monkeypatch.setattr(worker, "build_services", lambda *_args, **_kwargs: services)
    monkeypatch.setattr(worker, "close_services", _noop)
    services.settings.worker_poll_seconds = 0.01
    stop = asyncio.Event()

    task = asyncio.create_task(worker.run_worker(services.settings, stop))
    for _ in range(200):
        await asyncio.sleep(0.01)
        if worker._pending_count(services) == 0:
            break
    stop.set()
    await asyncio.wait_for(task, timeout=5)

    assert worker._pending_count(services) == 0
    document = services.database.fetch_one(
        "SELECT status, indexed_at FROM documents WHERE filename = ?",
        ("ghi-chu.txt",),
    )
    assert document is not None
    assert document["status"] == "ready"
    assert document["indexed_at"]


@pytest.mark.asyncio
async def test_the_worker_stops_when_asked(
    queue_only_client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Shutdown must not wait out a poll interval, or the launcher kills it instead."""
    services = queue_only_client.app.state.services
    monkeypatch.setattr(worker, "build_services", lambda *_args, **_kwargs: services)
    monkeypatch.setattr(worker, "close_services", _noop)
    services.settings.worker_poll_seconds = 30.0
    stop = asyncio.Event()

    task = asyncio.create_task(worker.run_worker(services.settings, stop))
    await asyncio.sleep(0.05)
    stop.set()

    await asyncio.wait_for(task, timeout=2)


@pytest.mark.asyncio
async def test_the_worker_waits_for_the_api_to_create_the_schema(tmp_path: Path) -> None:
    """The dev script starts both at once; a missing table must not kill the worker."""
    settings = Settings(
        data_dir=tmp_path / "empty",
        frontend_dist=tmp_path / "missing-web",
        worker_poll_seconds=0.05,
    )
    stop = asyncio.Event()

    task = asyncio.create_task(worker.run_worker(settings, stop))
    await asyncio.sleep(0.2)
    assert not task.done(), "the worker gave up instead of waiting for the schema"
    stop.set()

    await asyncio.wait_for(task, timeout=2)

async def _noop(*_args: object, **_kwargs: object) -> None:
    return None
