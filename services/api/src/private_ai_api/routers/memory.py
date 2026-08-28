from __future__ import annotations

from datetime import UTC, datetime
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, Response, status

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.routers.profiles import active_profile_id
from private_ai_api.schemas import MemoryCreate, MemoryRecord

router = APIRouter(prefix="/memory", tags=["memory"])


def _serialize(record: MemoryRecord) -> tuple[object, ...]:
    return (
        record.id,
        record.user_id,
        record.type.value,
        record.content,
        record.source,
        record.confidence,
        int(record.enabled),
        record.created_at.isoformat(),
        record.updated_at.isoformat(),
        record.expires_at.isoformat() if record.expires_at else None,
    )


def _to_record(row: dict[str, object]) -> MemoryRecord:
    return MemoryRecord(**{**row, "enabled": bool(row["enabled"])})


@router.get("", response_model=list[MemoryRecord])
def list_memories(
    services: Annotated[AppServices, Depends(get_services)],
    user_id: str | None = None,
    include_disabled: bool = False,
) -> list[MemoryRecord]:
    user_id = user_id or active_profile_id(services.database)
    predicate = "user_id = ?" if include_disabled else "user_id = ? AND enabled = 1"
    rows = services.database.fetch_all(
        f"SELECT * FROM memories WHERE {predicate} ORDER BY updated_at DESC",  # noqa: S608
        (user_id,),
    )
    return [_to_record(row) for row in rows]


@router.get("/search", response_model=list[MemoryRecord])
async def search_memories(
    q: str,
    services: Annotated[AppServices, Depends(get_services)],
    user_id: str | None = None,
    limit: int = 5,
) -> list[MemoryRecord]:
    if not q.strip():
        return []
    rows = await services.memory_service.search(
        q,
        user_id=user_id or active_profile_id(services.database),
        limit=limit,
    )
    return [_to_record(row) for row in rows]


@router.post("", response_model=MemoryRecord, status_code=status.HTTP_201_CREATED)
async def create_memory(
    payload: MemoryCreate,
    services: Annotated[AppServices, Depends(get_services)],
) -> MemoryRecord:
    record = MemoryRecord(
        **{**payload.model_dump(), "user_id": active_profile_id(services.database)}
    )
    services.database.execute(
        """
        INSERT INTO memories(
            id, user_id, type, content, source, confidence, enabled,
            created_at, updated_at, expires_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        _serialize(record),
    )
    await services.memory_service.sync_memory(record.id)
    return record


@router.patch("/{memory_id}", response_model=MemoryRecord)
async def update_memory(
    memory_id: str,
    payload: MemoryCreate,
    services: Annotated[AppServices, Depends(get_services)],
) -> MemoryRecord:
    existing = services.database.fetch_one("SELECT * FROM memories WHERE id = ?", (memory_id,))
    if not existing:
        raise HTTPException(status_code=404, detail="Memory not found")
    record = MemoryRecord(
        **{**payload.model_dump(), "user_id": str(existing["user_id"])},
        id=memory_id,
        enabled=bool(existing["enabled"]),
        created_at=existing["created_at"],
        updated_at=datetime.now(UTC),
    )
    services.database.execute(
        """
        UPDATE memories SET user_id=?, type=?, content=?, source=?, confidence=?,
        enabled=?, created_at=?, updated_at=?, expires_at=?,
        embedding_json=NULL, embedding_model=NULL WHERE id=?
        """,
        (*_serialize(record)[1:], memory_id),
    )
    await services.memory_service.sync_memory(memory_id)
    return record


@router.post("/{memory_id}/disable", response_model=MemoryRecord)
async def disable_memory(
    memory_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> MemoryRecord:
    now = datetime.now(UTC).isoformat()
    services.database.execute(
        "UPDATE memories SET enabled = 0, updated_at = ? WHERE id = ?", (now, memory_id)
    )
    row = services.database.fetch_one("SELECT * FROM memories WHERE id = ?", (memory_id,))
    if not row:
        raise HTTPException(status_code=404, detail="Memory not found")
    await services.memory_service.sync_memory(memory_id)
    return _to_record(row)


@router.post("/{memory_id}/enable", response_model=MemoryRecord)
async def enable_memory(
    memory_id: str,
    services: Annotated[AppServices, Depends(get_services)],
) -> MemoryRecord:
    now = datetime.now(UTC).isoformat()
    services.database.execute(
        "UPDATE memories SET enabled = 1, updated_at = ? WHERE id = ?", (now, memory_id)
    )
    row = services.database.fetch_one("SELECT * FROM memories WHERE id = ?", (memory_id,))
    if not row:
        raise HTTPException(status_code=404, detail="Memory not found")
    await services.memory_service.sync_memory(memory_id)
    return _to_record(row)


@router.delete("/{memory_id}", status_code=status.HTTP_204_NO_CONTENT)
async def forget_memory(
    memory_id: str,
    confirmed: bool,
    services: Annotated[AppServices, Depends(get_services)],
) -> Response:
    if not confirmed:
        raise HTTPException(status_code=409, detail="Forgetting memory requires confirmation")
    await services.memory_service.delete_memory(memory_id)
    return Response(status_code=status.HTTP_204_NO_CONTENT)
