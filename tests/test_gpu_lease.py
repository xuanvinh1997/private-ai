"""VRAM accounting. Ported from ``services/api/tests/test_gpu_lease.py``."""

from __future__ import annotations

import pytest

from private_ai.core.gpu_lease import GpuLeaseManager, InsufficientVram


async def test_gpu_lease_tracks_and_releases_capacity() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    async with manager.temporary("asr", 30):
        assert manager.reserved_bytes == 30
        with pytest.raises(InsufficientVram):
            await manager.reserve("llm", 71)
    assert manager.reserved_bytes == 0


async def test_a_lease_is_released_even_when_the_body_raises() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    with pytest.raises(RuntimeError):
        async with manager.temporary("asr", 30):
            raise RuntimeError("transcription failed")
    assert manager.reserved_bytes == 0


async def test_reserving_again_replaces_rather_than_stacks() -> None:
    """A model that reloads at a new size must not be counted twice."""
    manager = GpuLeaseManager(capacity_bytes=100)
    await manager.reserve("llm", 60)
    await manager.reserve("llm", 90)
    assert manager.reserved_bytes == 90


async def test_gpu_lease_synchronizes_observed_runtime_inventory() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    await manager.reserve("asr", 10)
    await manager.synchronize("ollama:", {"ollama:model-a": 40, "ollama:model-b": 20})
    assert manager.reserved_bytes == 70

    await manager.synchronize("ollama:", {"ollama:model-b": 25})
    assert manager.reserved_bytes == 35
    assert {lease["owner"] for lease in manager.snapshot()["leases"]} == {
        "asr",
        "ollama:model-b",
    }


async def test_synchronize_leaves_a_reservation_the_runtime_has_not_loaded_yet() -> None:
    """A model being loaded is reserved but not yet visible in ``/api/ps``."""
    manager = GpuLeaseManager(capacity_bytes=100)
    await manager.reserve("ollama:loading", 40)
    await manager.synchronize("ollama:", {})
    assert manager.owners("ollama:") == {"ollama:loading"}

    await manager.mark_observed("ollama:loading")
    await manager.synchronize("ollama:", {})
    # Once the runtime has confirmed it, its absence means it really is gone.
    assert manager.owners("ollama:") == set()


async def test_gpu_lease_rejects_inventory_outside_runtime_prefix() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    with pytest.raises(ValueError):
        await manager.synchronize("ollama:", {"asr": 10})


async def test_negative_reservations_are_refused() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    with pytest.raises(ValueError):
        await manager.reserve("asr", -1)
    with pytest.raises(ValueError):
        await manager.synchronize("ollama:", {"ollama:a": -1})


async def test_snapshot_reports_capacity_and_every_lease() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    await manager.reserve("asr", 25)
    snapshot = manager.snapshot()
    assert snapshot["capacity_bytes"] == 100
    assert snapshot["reserved_bytes"] == 25
    assert snapshot["leases"] == [{"owner": "asr", "bytes_reserved": 25, "source": "reserved"}]
