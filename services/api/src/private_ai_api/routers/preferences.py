from __future__ import annotations

from typing import Annotated

from fastapi import APIRouter, Depends

from private_ai_api.dependencies import AppServices, get_services
from private_ai_api.schemas import PreferencesRecord, PreferencesUpdate
from private_ai_api.services.document_processor import OCR_ENABLED_KEY

router = APIRouter(prefix="/preferences", tags=["preferences"])


def _current(services: AppServices) -> PreferencesRecord:
    return PreferencesRecord(ocr_enabled=services.document_processor.ocr_enabled())


@router.get("", response_model=PreferencesRecord)
def read_preferences(
    services: Annotated[AppServices, Depends(get_services)],
) -> PreferencesRecord:
    return _current(services)


@router.patch("", response_model=PreferencesRecord)
def update_preferences(
    payload: PreferencesUpdate,
    services: Annotated[AppServices, Depends(get_services)],
) -> PreferencesRecord:
    if payload.ocr_enabled is not None:
        services.database.execute(
            """
            INSERT INTO app_state(key, value) VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            """,
            (OCR_ENABLED_KEY, "1" if payload.ocr_enabled else "0"),
        )
    return _current(services)
