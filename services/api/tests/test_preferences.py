from __future__ import annotations

from io import BytesIO
from pathlib import Path
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient
from pypdf import PdfWriter

from private_ai_api.schemas import ModelInfo


def test_ocr_is_on_until_it_is_turned_off(client: TestClient) -> None:
    assert client.get("/api/v1/preferences").json() == {"ocr_enabled": True}

    turned_off = client.patch("/api/v1/preferences", json={"ocr_enabled": False})
    assert turned_off.status_code == 200
    assert turned_off.json() == {"ocr_enabled": False}
    assert client.get("/api/v1/preferences").json() == {"ocr_enabled": False}
    assert client.app.state.services.document_processor.ocr_enabled() is False


def test_turning_ocr_off_drops_the_markitdown_plugins(client: TestClient) -> None:
    processor = client.app.state.services.document_processor
    with_ocr = processor._markitdown_converter(True, "test-vision")

    # The converter is rebuilt rather than served from the cache built while OCR was on.
    assert processor._markitdown_converter(False, "test-vision") is not with_ocr
    assert processor._markitdown_converter(True, "test-vision") is not with_ocr


def test_a_scanned_pdf_reports_why_it_was_not_read(client: TestClient, tmp_path: Path) -> None:
    client.patch("/api/v1/preferences", json={"ocr_enabled": False})
    workspace = client.get("/api/v1/workspaces").json()[0]["id"]

    # A valid one-page PDF with no text layer, the shape a scan arrives in.
    blank = tmp_path / "scan.pdf"
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    with blank.open("wb") as handle:
        writer.write(handle)
    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("scan.pdf", blank.read_bytes(), "application/pdf")},
        data={"workspace_id": workspace},
    )
    assert uploaded.status_code == 201
    document = client.get(f"/api/v1/documents/{uploaded.json()['id']}").json()
    assert document["status"] == "needs_ocr"
    assert "OCR is off for this file" in document["error"]


def test_a_document_can_opt_into_ocr_against_the_default(client: TestClient, monkeypatch) -> None:
    """The default decides for a new upload; the document's own choice overrides it after."""
    client.patch("/api/v1/preferences", json={"ocr_enabled": False})
    processor = client.app.state.services.document_processor
    monkeypatch.setattr(processor, "vision_model", "test-vision")
    monkeypatch.setattr(
        processor,
        "_extract_markitdown",
        lambda _path, ocr, model="": (
            "*[Image OCR]\n\nChữ đọc được từ bản scan" if ocr else ""
        ),
    )

    workspace = client.get("/api/v1/workspaces").json()[0]["id"]
    blank = BytesIO()
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    writer.write(blank)

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("scan.pdf", blank.getvalue(), "application/pdf")},
        data={"workspace_id": workspace},
    )
    document_id = uploaded.json()["id"]
    assert client.get(f"/api/v1/documents/{document_id}").json()["status"] == "needs_ocr"

    retried = client.post(f"/api/v1/documents/{document_id}/process", params={"use_ocr": True})
    assert retried.status_code == 202
    document = client.get(f"/api/v1/documents/{document_id}").json()
    assert document["status"] == "ready"
    assert "Chữ đọc được" in document["extracted_text"]
    # The choice sticks, so a later re-read does not silently drop back to the default.
    assert processor.ocr_enabled(document_id) is True


def test_an_upload_can_opt_out_of_ocr_while_the_default_is_on(client: TestClient) -> None:
    workspace = client.get("/api/v1/workspaces").json()[0]["id"]
    blank = BytesIO()
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    writer.write(blank)

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("no-ocr.pdf", blank.getvalue(), "application/pdf")},
        data={"workspace_id": workspace, "use_ocr": "false"},
    )
    document = client.get(f"/api/v1/documents/{uploaded.json()['id']}").json()
    assert document["status"] == "needs_ocr"
    assert "OCR is off for this file" in document["error"]
    assert client.app.state.services.document_processor.ocr_enabled(document["id"]) is False


@pytest.mark.asyncio
async def test_ocr_picks_a_vision_model_without_being_told(client: TestClient) -> None:
    """Ticking OCR is the whole instruction; a capable model should not need a second pick."""
    processor = client.app.state.services.document_processor

    async def inventory() -> list[ModelInfo]:
        return [
            ModelInfo(name="qwen3:8b", model_type="language", capabilities=["chat"]),
            ModelInfo(name="gemma3:12b", model_type="language", capabilities=["chat", "vision"]),
        ]

    processor.ai = SimpleNamespace(list_models=inventory)
    assert await processor.resolve_vision_model() == "gemma3:12b"


@pytest.mark.asyncio
async def test_an_explicit_ocr_model_still_wins(client: TestClient) -> None:
    processor = client.app.state.services.document_processor
    client.app.state.services.database.execute(
        "INSERT INTO model_defaults(task, model_name, updated_at) VALUES ('vision', ?, '')",
        ("chosen-vision",),
    )

    async def inventory() -> list[ModelInfo]:
        return [ModelInfo(name="gemma3:12b", model_type="language", capabilities=["vision"])]

    processor.ai = SimpleNamespace(list_models=inventory)
    assert await processor.resolve_vision_model() == "chosen-vision"


@pytest.mark.asyncio
async def test_no_vision_model_anywhere_resolves_to_nothing(client: TestClient) -> None:
    processor = client.app.state.services.document_processor

    async def inventory() -> list[ModelInfo]:
        return [ModelInfo(name="qwen3:8b", model_type="language", capabilities=["chat"])]

    processor.ai = SimpleNamespace(list_models=inventory)
    assert await processor.resolve_vision_model() == ""
