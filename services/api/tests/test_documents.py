from importlib.metadata import entry_points
from io import BytesIO
from pathlib import Path
from types import SimpleNamespace

from fastapi.testclient import TestClient
from PIL import Image
from pypdf import PdfWriter


class FakeEmbedder:
    async def embed(self, _model: str, inputs: list[str]) -> list[list[float]]:
        vehicle_terms = ("motor vehicle", "automobile", "xe hơi", "ô tô")
        return [
            [1.0, 0.0] if any(term in value.casefold() for term in vehicle_terms) else [0.0, 1.0]
            for value in inputs
        ]


class FakeGraphExtractor:
    async def extract_graph(
        self,
        model: str,
        content: str,
    ) -> dict[str, list[dict[str, str]]]:
        assert model == "test-graph"
        assert "OpenAI" in content
        return {
            "entities": [
                {"key": "openai", "name": "OpenAI", "kind": "organization"},
                {"key": "san francisco", "name": "San Francisco", "kind": "place"},
            ],
            "relations": [
                {
                    "source_key": "openai",
                    "target_key": "san francisco",
                    "relation": "based_in",
                }
            ],
        }


workspace = {"workspace_id": "personal"}


def test_text_document_is_hashed_extracted_and_deduplicated(client: TestClient) -> None:
    files = {"file": ("notes.md", b"# Local knowledge\nXin chao", "text/markdown")}
    first = client.post("/api/v1/documents", files=files, data=workspace)
    second = client.post("/api/v1/documents", files=files, data=workspace)

    assert first.status_code == 201
    assert first.json()["status"] == "ready"
    assert first.json()["extracted_text"].startswith("# Local knowledge")
    assert second.status_code == 201
    assert second.json()["id"] == first.json()["id"]
    assert client.get("/api/v1/documents", params=workspace).json()["total"] == 1
    search = client.get(
        "/api/v1/documents/search",
        params={"q": "Local knowledge", "workspace_id": "personal"},
    )
    assert search.status_code == 200
    assert search.json()[0]["filename"] == "notes.md"
    assert "Xin chao" in search.json()[0]["content"]

    # The same bytes in another workspace are a separate document, not a dedup hit.
    other = client.post(
        "/api/v1/documents",
        files=files,
        data={"workspace_id": "research"},
    )
    assert other.status_code == 201
    assert other.json()["id"] != first.json()["id"]
    assert client.get("/api/v1/documents", params=workspace).json()["total"] == 1
    assert (
        client.get("/api/v1/documents", params={"workspace_id": "research"}).json()["total"] == 1
    )


def test_pdf_processing_status_retry_and_delete(client: TestClient) -> None:
    buffer = BytesIO()
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    writer.write(buffer)

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("scan.pdf", buffer.getvalue(), "application/pdf")},
        data={"workspace_id": "personal"},
    )
    assert uploaded.status_code == 201
    document_id = uploaded.json()["id"]

    document = client.get(f"/api/v1/documents/{document_id}")
    assert document.status_code == 200
    assert document.json()["status"] == "needs_ocr"

    retried = client.post(f"/api/v1/documents/{document_id}/process")
    assert retried.status_code == 202
    assert client.get(f"/api/v1/documents/{document_id}").json()["status"] == "needs_ocr"

    refused = client.delete(f"/api/v1/documents/{document_id}?confirmed=false")
    assert refused.status_code == 409
    deleted = client.delete(f"/api/v1/documents/{document_id}?confirmed=true")
    assert deleted.status_code == 204
    assert client.get(f"/api/v1/documents/{document_id}").status_code == 404


def test_scanned_pdf_uses_ocr_fallback(client: TestClient, monkeypatch) -> None:
    buffer = BytesIO()
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    writer.write(buffer)
    processor = client.app.state.services.document_processor
    monkeypatch.setattr(
        processor,
        "_ocr_pdf",
        lambda _path: "Nội dung nhận dạng từ trang scan",
    )

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("scanned.pdf", buffer.getvalue(), "application/pdf")},
        data={"workspace_id": "personal"},
    )

    assert uploaded.status_code == 201
    document = client.get(f"/api/v1/documents/{uploaded.json()['id']}").json()
    assert document["status"] == "ready"
    assert document["extracted_text"] == "Nội dung nhận dạng từ trang scan"
    search = client.get(
        "/api/v1/documents/search",
        params={"q": "nhận dạng trang scan", "workspace_id": "personal"},
    )
    assert search.status_code == 200
    assert search.json()[0]["filename"] == "scanned.pdf"


def _jpeg_bytes() -> bytes:
    output = BytesIO()
    Image.new("RGB", (32, 24), "white").save(output, format="JPEG")
    return output.getvalue()


def test_jpeg_uses_markitdown_vision_output(client: TestClient, monkeypatch) -> None:
    processor = client.app.state.services.document_processor

    class FakeVisionClient:
        def __init__(self, **_kwargs) -> None:
            self.chat = SimpleNamespace(
                completions=SimpleNamespace(
                    create=lambda **_request: SimpleNamespace(
                        choices=[
                            SimpleNamespace(
                                message=SimpleNamespace(content="Mã ảnh PRIVATE-JPG-4821")
                            )
                        ]
                    )
                )
            )

    processor.vision_model = "test-vision"
    monkeypatch.setattr(
        "private_ai_api.services.document_processor.OpenAI",
        FakeVisionClient,
    )

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("camera.jpg", _jpeg_bytes(), "image/jpeg")},
        data={"workspace_id": "personal"},
    )

    assert uploaded.status_code == 201
    document = client.get(f"/api/v1/documents/{uploaded.json()['id']}").json()
    assert document["status"] == "ready"
    assert "PRIVATE-JPG-4821" in document["extracted_text"]
    assert document["extracted_text"].startswith("<!-- private-ai-page:1 -->")


def test_jpeg_falls_back_to_local_ocr(client: TestClient, monkeypatch) -> None:
    processor = client.app.state.services.document_processor
    monkeypatch.setattr(processor, "_extract_markitdown", lambda _path: "")
    monkeypatch.setattr(
        processor,
        "_ocr_image",
        lambda _path: "<!-- private-ai-page:1 -->\n# Image OCR\n\nLOCAL-JPG-7319",
    )

    uploaded = client.post(
        "/api/v1/documents",
        files={"file": ("fallback.jpeg", _jpeg_bytes(), "image/jpeg")},
        data={"workspace_id": "personal"},
    )

    assert uploaded.status_code == 201
    document = client.get(f"/api/v1/documents/{uploaded.json()['id']}").json()
    assert document["status"] == "ready"
    assert "LOCAL-JPG-7319" in document["extracted_text"]
    results = client.get(
        "/api/v1/documents/search",
        params={"q": "LOCAL-JPG-7319", "workspace_id": "personal"},
    ).json()
    assert results[0]["filename"] == "fallback.jpeg"


def test_markitdown_ocr_plugin_is_installed() -> None:
    plugins = {plugin.name for plugin in entry_points(group="markitdown.plugin")}
    assert "ocr" in plugins


def test_pre_workspace_document_library_is_wiped_on_migration(tmp_path: Path) -> None:
    from private_ai_api.database import Database

    path = tmp_path / "legacy.db"
    database = Database(path)
    database.initialize()

    # Recreate the old global-library shape and drop a document into it.
    with database.connection() as connection:
        connection.execute("PRAGMA foreign_keys=OFF")
        connection.execute("DROP TABLE documents")
        connection.execute(
            """
            CREATE TABLE documents (
                id TEXT PRIMARY KEY, filename TEXT NOT NULL, media_type TEXT,
                sha256 TEXT NOT NULL UNIQUE, byte_size INTEGER NOT NULL,
                status TEXT NOT NULL, source_path TEXT NOT NULL, extracted_text TEXT,
                error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            )
            """
        )
    stale_dir = tmp_path / "stale-doc"
    stale_dir.mkdir()
    stale_file = stale_dir / "old.md"
    stale_file.write_text("# Old global document", encoding="utf-8")
    database.execute(
        """
        INSERT INTO documents(
            id, filename, media_type, sha256, byte_size, status, source_path,
            extracted_text, error, created_at, updated_at
        ) VALUES ('old-1', 'old.md', 'text/markdown', 'sha', 10, 'ready', ?, 'x', NULL, 'n', 'n')
        """,
        (str(stale_file),),
    )

    purged = Database(path).initialize()

    assert purged == [str(stale_file)]
    assert database.fetch_all("SELECT * FROM documents") == []
    columns = {row["name"] for row in database.fetch_all("PRAGMA table_info(documents)")}
    assert "workspace_id" in columns


def test_document_library_is_paginated(client: TestClient) -> None:
    for index in range(5):
        uploaded = client.post(
            "/api/v1/documents",
            files={"file": (f"doc-{index}.md", f"# Doc {index}".encode(), "text/markdown")},
            data=workspace,
        )
        assert uploaded.status_code == 201

    first = client.get("/api/v1/documents", params={**workspace, "limit": 2}).json()
    assert first["total"] == 5
    assert first["limit"] == 2
    assert first["offset"] == 0
    assert len(first["items"]) == 2

    second = client.get(
        "/api/v1/documents",
        params={**workspace, "limit": 2, "offset": 2},
    ).json()
    assert len(second["items"]) == 2
    assert {item["id"] for item in second["items"]}.isdisjoint({i["id"] for i in first["items"]})

    last = client.get("/api/v1/documents", params={**workspace, "limit": 2, "offset": 4}).json()
    assert len(last["items"]) == 1

    past_end = client.get("/api/v1/documents", params={**workspace, "limit": 2, "offset": 99})
    assert past_end.json()["items"] == []
    assert past_end.json()["total"] == 5

    # Out-of-range paging arguments are clamped, never fatal.
    clamped = client.get("/api/v1/documents", params={**workspace, "limit": 999, "offset": -5})
    assert clamped.status_code == 200
    assert clamped.json()["limit"] == 100
    assert clamped.json()["offset"] == 0


def test_document_library_filters_by_name_and_status(client: TestClient) -> None:
    for name in ("bao-cao-quy-1.md", "bao-cao-quy-2.md", "ghi-chu.md"):
        client.post(
            "/api/v1/documents",
            files={"file": (name, f"# {name}".encode(), "text/markdown")},
            data=workspace,
        )

    matched = client.get("/api/v1/documents", params={**workspace, "q": "bao-cao"}).json()
    assert matched["total"] == 2
    assert {item["filename"] for item in matched["items"]} == {
        "bao-cao-quy-1.md",
        "bao-cao-quy-2.md",
    }

    # Filtering narrows the page, but the workspace summary keeps counting everything.
    assert matched["summary"]["total"] == 3
    assert matched["summary"]["byte_size"] > 0
    assert matched["summary"]["pending"] == 0
    assert matched["summary"]["failed"] == 0

    by_status = client.get("/api/v1/documents", params={**workspace, "status": "ready"}).json()
    assert by_status["total"] == 3
    assert client.get(
        "/api/v1/documents",
        params={**workspace, "status": "failed"},
    ).json()["total"] == 0

    # Filter and pagination compose.
    paged = client.get(
        "/api/v1/documents",
        params={**workspace, "q": "bao-cao", "limit": 1, "offset": 1},
    ).json()
    assert paged["total"] == 2
    assert len(paged["items"]) == 1
