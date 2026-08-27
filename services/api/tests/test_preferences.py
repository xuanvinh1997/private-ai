from __future__ import annotations

from pathlib import Path

from fastapi.testclient import TestClient
from pypdf import PdfWriter


def test_ocr_is_on_until_it_is_turned_off(client: TestClient) -> None:
    assert client.get("/api/v1/preferences").json() == {"ocr_enabled": True}

    turned_off = client.patch("/api/v1/preferences", json={"ocr_enabled": False})
    assert turned_off.status_code == 200
    assert turned_off.json() == {"ocr_enabled": False}
    assert client.get("/api/v1/preferences").json() == {"ocr_enabled": False}
    assert client.app.state.services.document_processor.ocr_enabled() is False


def test_turning_ocr_off_drops_the_markitdown_plugins(client: TestClient) -> None:
    processor = client.app.state.services.document_processor
    with_ocr = processor._markitdown_converter()

    client.patch("/api/v1/preferences", json={"ocr_enabled": False})
    without_ocr = processor._markitdown_converter()

    # The converter is rebuilt rather than served from the cache built while OCR was on.
    assert without_ocr is not with_ocr


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
    assert "OCR is turned off" in document["error"]
