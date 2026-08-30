"""What this machine and this moment are — read locally, sent nowhere."""

from __future__ import annotations

import platform
import re
from datetime import datetime
from pathlib import Path

from private_ai.config import Settings
from private_ai.core.gpu_lease import GpuLeaseManager
from private_ai.core.system_info import machine_snapshot, time_snapshot


def test_machine_snapshot_reports_the_host_it_runs_on(settings: Settings) -> None:
    snapshot = machine_snapshot(settings)

    assert snapshot["app"]["name"] == settings.app_name
    assert snapshot["app"]["data_dir"] == str(settings.data_dir)
    assert snapshot["os"]["system"] == platform.system()
    assert snapshot["python"]["version"] == platform.python_version()
    assert snapshot["cpu"]["logical_cores"] is None or snapshot["cpu"]["logical_cores"] > 0
    assert isinstance(snapshot["memory"]["unified_with_gpu"], bool)


async def test_gpu_section_reflects_live_leases_when_a_manager_is_given(
    settings: Settings,
) -> None:
    without = machine_snapshot(settings)
    assert without["gpu"]["reserved_bytes"] == 0
    assert without["gpu"]["capacity_bytes"] == settings.gpu_capacity_bytes

    leases = GpuLeaseManager(capacity_bytes=1000)
    await leases.reserve("asr", 250)

    with_leases = machine_snapshot(settings, leases)
    assert with_leases["gpu"]["capacity_bytes"] == 1000
    assert with_leases["gpu"]["reserved_bytes"] == 250
    assert with_leases["gpu"]["leases"][0]["owner"] == "asr"


def test_disk_usage_degrades_to_an_error_rather_than_raising(tmp_path: Path) -> None:
    missing = Settings(data_dir=tmp_path / "never-created")
    snapshot = machine_snapshot(missing)
    disk = snapshot["disk"]
    assert disk["path"] == str(missing.data_dir)
    assert "error" in disk or "free_bytes" in disk


def test_time_snapshot_agrees_with_itself() -> None:
    """The model cannot know today's date; these fields are the only source it has."""
    snapshot = time_snapshot()

    local = datetime.fromisoformat(str(snapshot["local_iso"]))
    utc = datetime.fromisoformat(str(snapshot["utc_iso"]))
    assert abs((local - utc).total_seconds()) < 1

    assert str(snapshot["date"]) == local.date().isoformat()
    assert str(snapshot["time"]) == local.strftime("%H:%M:%S")
    assert snapshot["weekday"] == local.strftime("%A")
    assert re.fullmatch(r"\d{4}-W\d{2}", str(snapshot["iso_week"]))
    assert snapshot["utc_offset_minutes"] == int(
        (local.utcoffset().total_seconds() // 60) if local.utcoffset() else 0
    )
    assert abs(int(snapshot["unix_seconds"]) - local.timestamp()) < 2
