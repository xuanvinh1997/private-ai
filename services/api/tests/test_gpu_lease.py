import pytest

from private_ai_api.services.gpu_lease import GpuLeaseManager, InsufficientVram


@pytest.mark.asyncio
async def test_gpu_lease_tracks_and_releases_capacity() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    async with manager.temporary("asr", 30):
        assert manager.reserved_bytes == 30
        with pytest.raises(InsufficientVram):
            await manager.reserve("llm", 71)
    assert manager.reserved_bytes == 0


@pytest.mark.asyncio
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


@pytest.mark.asyncio
async def test_gpu_lease_rejects_inventory_outside_runtime_prefix() -> None:
    manager = GpuLeaseManager(capacity_bytes=100)
    with pytest.raises(ValueError):
        await manager.synchronize("ollama:", {"asr": 10})
