from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import asdict, dataclass


class InsufficientVram(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class Lease:
    owner: str
    bytes_reserved: int
    source: str = "reserved"


class GpuLeaseManager:
    """Coordinates model reservations without depending on a vendor CLI."""

    def __init__(self, capacity_bytes: int) -> None:
        self.capacity_bytes = capacity_bytes
        self._leases: dict[str, Lease] = {}
        self._lock = asyncio.Lock()

    @property
    def reserved_bytes(self) -> int:
        return sum(lease.bytes_reserved for lease in self._leases.values())

    async def reserve(self, owner: str, bytes_required: int) -> Lease:
        if bytes_required < 0:
            raise ValueError("bytes_required cannot be negative")
        async with self._lock:
            existing = self._leases.get(owner)
            used_without_owner = self.reserved_bytes - (existing.bytes_reserved if existing else 0)
            if used_without_owner + bytes_required > self.capacity_bytes:
                raise InsufficientVram(f"Cannot reserve {bytes_required} bytes for {owner}")
            lease = Lease(owner=owner, bytes_reserved=bytes_required)
            self._leases[owner] = lease
            return lease

    async def release(self, owner: str) -> None:
        async with self._lock:
            self._leases.pop(owner, None)

    async def synchronize(self, prefix: str, reservations: dict[str, int]) -> None:
        """Replace observed leases for one runtime with its authoritative inventory."""
        if any(value < 0 for value in reservations.values()):
            raise ValueError("reservation bytes cannot be negative")
        async with self._lock:
            for owner, lease in tuple(self._leases.items()):
                if (
                    owner.startswith(prefix)
                    and owner not in reservations
                    and lease.source == "observed"
                ):
                    self._leases.pop(owner, None)
            for owner, bytes_reserved in reservations.items():
                if not owner.startswith(prefix):
                    raise ValueError(f"reservation owner must start with {prefix!r}")
                self._leases[owner] = Lease(
                    owner=owner,
                    bytes_reserved=bytes_reserved,
                    source="observed",
                )

    async def mark_observed(self, owner: str) -> None:
        async with self._lock:
            lease = self._leases.get(owner)
            if lease:
                self._leases[owner] = Lease(
                    owner=owner,
                    bytes_reserved=lease.bytes_reserved,
                    source="observed",
                )

    def owners(self, prefix: str = "") -> set[str]:
        return {owner for owner in self._leases if owner.startswith(prefix)}

    @asynccontextmanager
    async def temporary(self, owner: str, bytes_required: int) -> AsyncIterator[Lease]:
        lease = await self.reserve(owner, bytes_required)
        try:
            yield lease
        finally:
            await self.release(owner)

    def snapshot(self) -> dict[str, object]:
        return {
            "capacity_bytes": self.capacity_bytes,
            "reserved_bytes": self.reserved_bytes,
            "leases": [asdict(lease) for lease in self._leases.values()],
        }
