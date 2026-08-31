"""Files the agent produces.

Two things are worth asserting here and they are different in kind. One is that each
builder emits a file the right program can open — a .docx that python-docx reads back, a
page whose data really is the data that went in. The other is that a spec which would
produce a *wrong* picture is refused rather than drawn: a series one value short of its
labels renders as a plausible chart of the wrong numbers, and nothing downstream would
ever catch it.
"""

from __future__ import annotations

import io
import json
import re
from pathlib import Path

import pytest

from private_ai.core.artifacts import (
    ArtifactError,
    ArtifactStore,
    ChartCandle,
    ChartSeries,
    ChartSpec,
    DiagramSpec,
    DocumentBlock,
    DocumentSpec,
    SlideSpec,
    SlidesSpec,
    build_docx,
    build_pptx,
    render_chart_page,
    render_diagram_page,
    slugify,
)
from private_ai.core.services import AppServices
from private_ai.mcp.adapter import alias_for, invoker
from private_ai.mcp.client import AGENT_TOOLS, ARTIFACT_TOOLS, READ_ONLY_TOOLS
from private_ai.mcp.servers import artifacts as artifacts_server


@pytest.fixture
def store(tmp_path: Path) -> ArtifactStore:
    return ArtifactStore(tmp_path / "artifacts")


# --- the store ------------------------------------------------------------


def test_a_vietnamese_title_becomes_a_plain_filename() -> None:
    assert slugify("Báo cáo Quý 1/2026") == "bao-cao-quy-1-2026"
    assert slugify("Đường đi của dữ liệu") == "duong-di-cua-du-lieu"
    # A title with nothing to slugify still has to produce a name.
    assert slugify("···") == "artifact"


def test_two_writes_of_the_same_title_never_overwrite_each_other(store: ArtifactStore) -> None:
    first = store.write_text("", "Báo cáo", ".html", "một")
    second = store.write_text("", "Báo cáo", ".html", "hai")
    assert first != second
    assert first.read_text(encoding="utf-8") == "một"
    assert second.read_text(encoding="utf-8") == "hai"


def test_a_workspace_id_that_is_not_a_plain_token_is_refused(store: ArtifactStore) -> None:
    """The id becomes a directory name, so this is the path-traversal boundary."""
    for bad in ("../escape", "a/b", ".hidden", "has space"):
        with pytest.raises(ArtifactError):
            store.folder(bad)


def test_files_are_listed_newest_first_and_typed_by_suffix(store: ArtifactStore) -> None:
    store.write_text("", "Sơ đồ", ".mmd", "flowchart TB\n  A --> B\n")
    store.write_bytes("", "Bản trình chiếu", ".pptx", b"stub")
    kinds = {item.kind for item in store.listing()}
    assert kinds == {"diagram-source", "slides"}


# --- charts ---------------------------------------------------------------


def _spec_json(html: str) -> dict:
    """Pull the spec back out of the page, to assert on what the browser will read."""
    matched = re.search(r"var SPEC = (\{.*?\});\n", html, re.S)
    assert matched, "trang không nhúng được SPEC"
    return json.loads(matched.group(1).replace("\\u003c", "<").replace("\\u003e", ">"))


def test_a_chart_page_carries_its_data_and_needs_no_network() -> None:
    html = render_chart_page(
        ChartSpec(
            title="Doanh thu theo quý",
            chart_type="bar",
            categories=["Q1", "Q2"],
            series=[ChartSeries("2026", [120.0, 140.0])],
            unit="tỷ VND",
            source="bao-cao.xlsx",
        )
    )
    payload = _spec_json(html)
    assert payload["categories"] == ["Q1", "Q2"]
    assert payload["series"][0]["values"] == [120.0, 140.0]
    # The whole point of hand-rolling the renderer: nothing is fetched when it opens.
    assert "http://" not in html and "https://" not in html
    assert "bao-cao.xlsx" in html


def test_a_series_that_does_not_match_its_labels_is_refused() -> None:
    """Padding it would silently shift every point and draw the wrong chart."""
    with pytest.raises(ArtifactError, match="nhãn"):
        render_chart_page(
            ChartSpec(
                title="Lệch",
                categories=["Q1", "Q2", "Q3"],
                series=[ChartSeries("2026", [1.0, 2.0])],
            )
        )


def test_a_candle_whose_high_is_not_the_highest_is_refused() -> None:
    with pytest.raises(ArtifactError, match="high/low"):
        render_chart_page(
            ChartSpec(
                title="Giá",
                chart_type="candlestick",
                candles=[ChartCandle("02/01", open=10, high=9, low=8, close=11)],
            )
        )


def test_a_pie_takes_exactly_one_series() -> None:
    with pytest.raises(ArtifactError, match="tròn"):
        render_chart_page(
            ChartSpec(
                title="Cơ cấu",
                chart_type="pie",
                categories=["A", "B"],
                series=[ChartSeries("x", [1.0, 2.0]), ChartSeries("y", [3.0, 4.0])],
            )
        )


def test_a_title_holding_html_cannot_break_out_of_the_page() -> None:
    html = render_chart_page(
        ChartSpec(
            title='</script><img src=x onerror="alert(1)">',
            categories=["A"],
            series=[ChartSeries("</script>", [1.0])],
        )
    )
    assert "<img src=x" not in html
    assert "</script><img" not in html


# --- diagrams -------------------------------------------------------------


def test_a_fenced_block_is_unwrapped_rather_than_refused() -> None:
    spec = DiagramSpec(title="Kiến trúc", source="```mermaid\nflowchart TB\n  A --> B\n```")
    assert spec.cleaned() == "flowchart TB\n  A --> B"


def test_prose_where_mermaid_belongs_is_refused_with_an_example() -> None:
    with pytest.raises(ArtifactError, match="flowchart TB"):
        render_diagram_page(DiagramSpec(title="Sơ đồ", source="Hệ thống gồm ba phần."))


def test_the_diagram_page_still_reads_when_the_library_never_loads() -> None:
    """The CDN is the only network dependency in the package, so its failure is designed
    for rather than merely handled: the source is in the page as text."""
    html = render_diagram_page(DiagramSpec(title="Kiến trúc", source="flowchart TB\n  UI --> API"))
    assert "UI --&gt; API" in html
    assert "Không vẽ được sơ đồ" in html
    # A static import would abort the module and skip the catch that shows that message.
    assert "import mermaid" not in html
    assert "await import(CDN)" in html or "import(CDN)" in html


# --- Word and PowerPoint --------------------------------------------------


def test_a_document_opens_as_a_real_docx_with_its_content_in_order() -> None:
    docx = pytest.importorskip("docx")
    payload = build_docx(
        DocumentSpec(
            title="Báo cáo quý 1",
            subtitle="Nội bộ",
            blocks=[
                DocumentBlock(type="paragraph", text="Doanh thu tăng 12%."),
                DocumentBlock(type="heading", text="Số liệu", level=1),
                DocumentBlock(type="bullets", items=["Một", "  Chi tiết", "Hai"]),
                DocumentBlock(
                    type="table", rows=[["Kỳ", "Doanh thu"], ["Q1", "540"]], text="Bảng 1"
                ),
            ],
        )
    )
    document = docx.Document(io.BytesIO(payload))
    text = [paragraph.text for paragraph in document.paragraphs]
    assert "Báo cáo quý 1" in text
    assert text.index("Doanh thu tăng 12%.") < text.index("Số liệu")
    assert document.tables[0].rows[1].cells[1].text == "540"


def test_a_ragged_table_is_refused_naming_the_row() -> None:
    with pytest.raises(ArtifactError, match="dòng 2"):
        build_docx(
            DocumentSpec(
                title="x",
                blocks=[DocumentBlock(type="table", rows=[["a", "b"], ["c"]])],
            )
        )


def test_a_deck_is_sixteen_by_nine_and_leads_with_a_title_slide() -> None:
    pptx = pytest.importorskip("pptx")
    payload = build_pptx(
        SlidesSpec(
            title="Kế hoạch 2026",
            subtitle="Ban giám đốc",
            slides=[
                SlideSpec(layout="section", title="Bối cảnh"),
                SlideSpec(
                    layout="bullets",
                    title="Ba việc",
                    bullets=["Một", "  Chi tiết"],
                    notes="Nói chậm ở đây.",
                ),
            ],
        )
    )
    deck = pptx.Presentation(io.BytesIO(payload))
    ratio = deck.slide_width / deck.slide_height
    assert abs(ratio - 16 / 9) < 0.01
    assert deck.slides[0].shapes.title.text == "Kế hoạch 2026"
    assert deck.slides[1].shapes.title.text == "Bối cảnh"
    assert deck.slides[2].notes_slide.notes_text_frame.text == "Nói chậm ở đây."
    # Two leading spaces is the whole nesting vocabulary, and it has to survive.
    body = next(p for p in deck.slides[2].placeholders if p.placeholder_format.idx != 0)
    assert [(p.text, p.level) for p in body.text_frame.paragraphs] == [("Một", 0), ("Chi tiết", 1)]


def test_a_slide_layout_that_needs_two_columns_says_so_when_given_one() -> None:
    with pytest.raises(ArtifactError, match="two_column"):
        build_pptx(
            SlidesSpec(
                title="x",
                slides=[SlideSpec(layout="two_column", title="So sánh", left=["A"])],
            )
        )


# --- the server -----------------------------------------------------------


async def test_the_tools_write_into_the_workspace_folder(
    services: AppServices, workspace_id: str
) -> None:
    server = artifacts_server.create_server(services)
    result = await server.call_tool(
        "artifacts.create_chart",
        {
            "title": "Doanh thu",
            "chart_type": "line",
            "categories": ["Q1", "Q2"],
            "series": [{"name": "2026", "values": [1.0, 2.0]}],
            "workspace_id": workspace_id,
        },
    )
    path = Path(result.structured_content["path"])
    assert path.is_file()
    assert path.parent.name == workspace_id
    assert path.parent.parent == services.settings.artifacts_dir


async def test_a_diagram_leaves_its_source_beside_the_page(
    services: AppServices, workspace_id: str
) -> None:
    server = artifacts_server.create_server(services)
    await server.call_tool(
        "artifacts.create_diagram",
        {
            "title": "Kiến trúc",
            "source": "flowchart TB\n  A --> B",
            "workspace_id": workspace_id,
        },
    )
    written = {path.suffix for path in services.artifacts.folder(workspace_id).iterdir()}
    assert written == {".html", ".mmd"}


async def test_a_bad_spec_comes_back_as_readable_tool_text_not_a_dead_turn(
    services: AppServices, workspace_id: str
) -> None:
    """A model reads this string and retries; an exception just ends the turn silently."""
    server = artifacts_server.create_server(services)
    call = invoker(server, alias_for("artifacts.create_chart"), allow=AGENT_TOOLS)
    answer = await call(
        title="Lệch",
        chart_type="bar",
        categories=["Q1", "Q2", "Q3"],
        series=[{"name": "2026", "values": [1.0]}],
        workspace_id=workspace_id,
    )
    assert "nhãn" in answer
    assert not list(services.artifacts.folder(workspace_id).iterdir())


async def test_an_invented_workspace_id_writes_no_folder(services: AppServices) -> None:
    server = artifacts_server.create_server(services)
    call = invoker(server, alias_for("artifacts.create_diagram"), allow=AGENT_TOOLS)
    answer = await call(
        title="Kiến trúc", source="flowchart TB\n  A --> B", workspace_id="khong-co-that"
    )
    assert "workspaces.list" in answer
    assert not (services.settings.artifacts_dir / "khong-co-that").exists()


def test_the_artifact_tools_are_the_only_writers_the_agent_is_handed() -> None:
    """They are outside the read-only set by definition, and inside the agent's set on
    purpose: every one creates a new file under ``artifacts/`` and nothing else."""
    assert not ARTIFACT_TOOLS & READ_ONLY_TOOLS
    assert ARTIFACT_TOOLS < AGENT_TOOLS
    assert all(name.startswith("artifacts.") for name in ARTIFACT_TOOLS)
    # Nothing that opens, overwrites, moves or deletes. Adding one is a decision, not a
    # detail, so it has to break this test first.
    assert not any(
        word in name for name in ARTIFACT_TOOLS for word in ("open", "delete", "write", "run")
    )
