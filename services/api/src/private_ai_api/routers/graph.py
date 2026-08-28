from __future__ import annotations

from typing import Annotated, Any

from fastapi import APIRouter, Depends, HTTPException

from private_ai_api.dependencies import AppServices, get_services

router = APIRouter(prefix="/graph", tags=["graph"])


def _require_workspace(services: AppServices, workspace_id: str) -> None:
    workspace = services.database.fetch_one(
        "SELECT id FROM workspaces WHERE id = ?",
        (workspace_id,),
    )
    if not workspace:
        raise HTTPException(status_code=404, detail="Workspace not found")


@router.get("")
async def read_graph(
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
    entity: str = "*",
    depth: int = 2,
    limit: int = 150,
) -> dict[str, Any]:
    """Nodes and edges of one workspace, ready to be drawn."""
    _require_workspace(services, workspace_id)
    return await services.lightrag.knowledge_graph(
        workspace_id,
        entity=entity,
        depth=depth,
        limit=limit,
    )


@router.get("/entities")
async def list_entities(
    workspace_id: str,
    services: Annotated[AppServices, Depends(get_services)],
    q: str = "",
    limit: int = 50,
) -> list[dict[str, object]]:
    """Entity labels the workspace knows about, for the graph search box."""
    _require_workspace(services, workspace_id)
    return await services.lightrag.find_entities(q, workspace_id, max(1, min(limit, 100)))
