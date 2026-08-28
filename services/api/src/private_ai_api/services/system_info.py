from __future__ import annotations

import os
import platform
import shutil
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from private_ai_api.config import (
    Settings,
    is_unified_memory,
    total_memory_bytes,
)
from private_ai_api.services.gpu_lease import GpuLeaseManager


def machine_snapshot(
    settings: Settings,
    gpu_leases: GpuLeaseManager | None = None,
) -> dict[str, Any]:
    """What this machine is, for a model that would otherwise have to guess.

    Everything here is read locally; nothing about the host is sent anywhere by this call.
    """
    disk = _disk_usage(settings.data_dir)
    return {
        "app": {"name": settings.app_name, "data_dir": str(settings.data_dir)},
        "os": {
            "system": platform.system(),
            "release": platform.release(),
            "version": platform.version(),
            "machine": platform.machine(),
            "hostname": platform.node(),
        },
        "python": {
            "version": platform.python_version(),
            "implementation": platform.python_implementation(),
            "executable": sys.executable,
        },
        "cpu": {
            "logical_cores": os.cpu_count(),
            "processor": platform.processor() or platform.machine(),
        },
        "memory": {
            "total_bytes": total_memory_bytes(),
            # Apple Silicon has no separate VRAM: the GPU draws from the same pool.
            "unified_with_gpu": is_unified_memory(),
        },
        "gpu": (
            gpu_leases.snapshot()
            if gpu_leases
            else {"capacity_bytes": settings.gpu_capacity_bytes, "reserved_bytes": 0, "leases": []}
        ),
        "disk": disk,
    }


def _disk_usage(target: Path) -> dict[str, Any]:
    try:
        usage = shutil.disk_usage(target)
    except OSError as exc:
        return {"path": str(target), "error": str(exc)}
    return {
        "path": str(target),
        "total_bytes": usage.total,
        "used_bytes": usage.used,
        "free_bytes": usage.free,
    }


def time_snapshot() -> dict[str, Any]:
    """The current date and time, which a model's training data cannot supply."""
    local = datetime.now().astimezone()
    utc = local.astimezone(UTC)
    offset = local.utcoffset()
    return {
        "local_iso": local.isoformat(),
        "utc_iso": utc.isoformat(),
        "date": local.date().isoformat(),
        "time": local.strftime("%H:%M:%S"),
        "timezone": local.tzname() or "",
        "utc_offset_minutes": int(offset.total_seconds() // 60) if offset else 0,
        "unix_seconds": int(local.timestamp()),
        "weekday": local.strftime("%A"),
        "iso_week": local.strftime("%G-W%V"),
    }
