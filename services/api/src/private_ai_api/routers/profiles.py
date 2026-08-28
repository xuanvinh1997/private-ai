from __future__ import annotations

from datetime import UTC, datetime
from typing import Annotated, Any
from uuid import uuid4

from fastapi import APIRouter, Depends, HTTPException, Response, status

from private_ai_api.database import ACTIVE_PROFILE_KEY, Database
from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import ProfileCreate, ProfileRecord, ProfileUpdate

router = APIRouter(prefix="/profiles", tags=["profiles"])

PROFILE_COLUMNS = """
    SELECT p.*, COUNT(m.id) AS memory_count
    FROM profiles p
    LEFT JOIN memories m ON m.user_id = p.id
"""


def active_profile_id(database: Database) -> str:
    """The profile every unattributed read and write belongs to.

    Falls back to the oldest profile so a stale or missing pointer cannot leave the app
    without an identity; the pointer is repaired on the next activation.
    """
    stored = database.fetch_one(
        "SELECT value FROM app_state WHERE key = ?",
        (ACTIVE_PROFILE_KEY,),
    )
    if stored:
        candidate = str(stored["value"])
        if database.fetch_one("SELECT 1 FROM profiles WHERE id = ?", (candidate,)):
            return candidate
    row = database.fetch_one("SELECT id FROM profiles ORDER BY created_at ASC LIMIT 1")
    return str(row["id"]) if row else ""


def _record(row: dict[str, Any], active_id: str) -> ProfileRecord:
    return ProfileRecord(**row, active=str(row["id"]) == active_id)


def _fetch(services: AppServices, profile_id: str) -> ProfileRecord:
    row = services.database.fetch_one(
        f"{PROFILE_COLUMNS} WHERE p.id = ? GROUP BY p.id",
        (profile_id,),
    )
    if not row:
        raise HTTPException(status_code=404, detail="Profile not found")
    return _record(row, active_profile_id(services.database))


@router.get("", response_model=list[ProfileRecord])
def list_profiles(
    services: Annotated[AppServices, Depends(get_services)],
) -> list[ProfileRecord]:
    active_id = active_profile_id(services.database)
    rows = services.database.fetch_all(f"{PROFILE_COLUMNS} GROUP BY p.id ORDER BY p.created_at ASC")
    return [_record(row, active_id) for row in rows]


@router.get("/active", response_model=ProfileRecord)
def read_active_profile(
    services: Annotated[AppServices, Depends(get_services)],
) -> ProfileRecord:
    profile_id = active_profile_id(services.database)
    if not profile_id:
        raise HTTPException(status_code=404, detail="Profile not found")
    return _fetch(services, profile_id)


@router.post("", response_model=ProfileRecord, status_code=status.HTTP_201_CREATED)
def create_profile(
    payload: ProfileCreate,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProfileRecord:
    """Add a profile and switch to it, which is the only reason to add one."""
    profile_id = str(uuid4())
    now = datetime.now(UTC).isoformat()
    services.database.execute(
        "INSERT INTO profiles(id, display_name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        (profile_id, payload.display_name.strip(), now, now),
    )
    _activate(services, profile_id)
    return _fetch(services, profile_id)


@router.patch("/{profile_id}", response_model=ProfileRecord)
def update_profile(
    profile_id: str,
    payload: ProfileUpdate,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProfileRecord:
    existing = services.database.fetch_one("SELECT id FROM profiles WHERE id = ?", (profile_id,))
    if not existing:
        raise HTTPException(status_code=404, detail="Profile not found")
    services.database.execute(
        "UPDATE profiles SET display_name = ?, updated_at = ? WHERE id = ?",
        (payload.display_name.strip(), datetime.now(UTC).isoformat(), profile_id),
    )
    return _fetch(services, profile_id)


@router.post("/{profile_id}/activate", response_model=ProfileRecord)
def activate_profile(
    profile_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> ProfileRecord:
    existing = services.database.fetch_one("SELECT id FROM profiles WHERE id = ?", (profile_id,))
    if not existing:
        raise HTTPException(status_code=404, detail="Profile not found")
    _activate(services, profile_id)
    return _fetch(services, profile_id)


@router.delete("/{profile_id}", status_code=status.HTTP_204_NO_CONTENT)
def delete_profile(
    profile_id: str,
    confirmed: bool,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    """Remove a profile and everything it remembers.

    Workspaces, conversations and documents are shared by everyone on this machine, so
    they are left alone; only the memories written under this profile go with it.
    """
    if not confirmed:
        raise HTTPException(status_code=409, detail="Profile deletion requires confirmation")
    existing = services.database.fetch_one("SELECT id FROM profiles WHERE id = ?", (profile_id,))
    if not existing:
        raise HTTPException(status_code=404, detail="Profile not found")
    remaining = services.database.fetch_all(
        "SELECT id FROM profiles WHERE id != ? ORDER BY created_at ASC",
        (profile_id,),
    )
    if not remaining:
        raise HTTPException(status_code=409, detail="The last profile cannot be deleted")
    services.database.execute("DELETE FROM memories WHERE user_id = ?", (profile_id,))
    services.database.execute("DELETE FROM profiles WHERE id = ?", (profile_id,))
    if active_profile_id(services.database) == profile_id:
        _activate(services, str(remaining[0]["id"]))
    return Response(status_code=status.HTTP_204_NO_CONTENT)


def _activate(services: AppServices, profile_id: str) -> None:
    services.database.execute(
        """
        INSERT INTO app_state(key, value) VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        """,
        (ACTIVE_PROFILE_KEY, profile_id),
    )
