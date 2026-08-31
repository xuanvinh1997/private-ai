"""Files the agent makes: charts, diagrams, Word documents and decks.

This is the first server in Private AI that writes anything. It is allowed to because of
what it can and cannot reach: every path it produces is a *new* file under
``data_dir/artifacts``, named with a timestamp, and there is no tool here that opens,
overwrites, moves or deletes one. A model can add to that folder and nothing else — it
cannot touch the user's own files, the document library, or its own past output.

Nothing is launched either. The tools return a path and stop; opening the result is the
user's action, taken deliberately, on a file they can look at first.

The heavy lifting is in :mod:`private_ai.core.artifacts`. What lives here is the schema a
model fills in and the error text it reads when it fills it in wrong.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Literal

from mcp.server.mcpserver import MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from pydantic import BaseModel, Field

from private_ai.core.artifacts import (
    BLOCK_TYPES,
    CHART_TYPES,
    SLIDE_LAYOUTS,
    VALUE_FORMATS,
    ArtifactError,
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
)
from private_ai.mcp.common import build_server, require_workspace, resolve_services, stdio_entry

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from pathlib import Path

    from private_ai.core.artifacts import ArtifactStore
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-artifacts"

INSTRUCTIONS = (
    "Turn an answer into a file: an interactive chart, a Mermaid diagram, a Word "
    "document or a PowerPoint deck. Everything is written locally under the app's "
    "artifacts folder and nothing is sent anywhere.\n\n"
    "Use these when the user asked for a file, a chart, a diagram or a deck — not to "
    "decorate an answer that is already complete as text. A number the user can read in "
    "one sentence does not need a chart, and three bullet points do not need a deck.\n\n"
    "Every tool returns the path it wrote and stops there. Nothing is opened: tell the "
    "user where the file is and let them open it."
)

# What the model is told about the whole family, repeated on each tool because the tool
# description is the only text read at the moment of choosing.
_SHARED_RULES = (
    "Chỉ dùng số liệu có thật trong tài liệu, trong câu hỏi hoặc do người dùng cung cấp; "
    "không bịa số để lấp chỗ trống. Trả về đường dẫn tệp đã ghi — hãy nêu đường dẫn đó "
    "cho người dùng, đừng cố mở tệp."
)


class SeriesInput(BaseModel):
    name: str = Field(description="Tên chuỗi, hiện trong chú giải và tooltip")
    values: list[float] = Field(
        description="Các giá trị, cùng thứ tự và cùng độ dài với 'categories'"
    )


class CandleInput(BaseModel):
    label: str = Field(description="Nhãn phiên, thường là ngày: '02/01/2026'")
    open: float
    high: float
    low: float
    close: float
    volume: float | None = Field(default=None, description="Khối lượng, vẽ ở dải dưới nếu có")


class BlockInput(BaseModel):
    type: Literal[BLOCK_TYPES] = Field(  # type: ignore[valid-type]
        description=(
            "heading (tiêu đề mục) · paragraph (đoạn văn) · bullets (gạch đầu dòng) · "
            "numbered (danh sách đánh số) · table (bảng) · quote (trích dẫn) · "
            "code (khối mã) · page_break (ngắt trang)"
        )
    )
    text: str = Field(default="", description="Nội dung cho heading/paragraph/quote/code")
    level: int = Field(default=1, ge=1, le=4, description="Cấp tiêu đề, chỉ dùng với heading")
    items: list[str] = Field(
        default_factory=list,
        description="Các dòng cho bullets/numbered. Thụt đầu dòng hai dấu cách để lùi một cấp.",
    )
    rows: list[list[str]] = Field(
        default_factory=list,
        description="Bảng theo dòng; dòng đầu tiên là tiêu đề cột. Mọi dòng phải cùng số ô.",
    )


class SlideInput(BaseModel):
    layout: Literal[SLIDE_LAYOUTS] = Field(  # type: ignore[valid-type]
        default="bullets",
        description=(
            "bullets (tiêu đề + gạch đầu dòng) · two_column (hai cột) · "
            "section (slide phân đoạn) · title (slide tiêu đề) · quote (câu trích) · blank"
        ),
    )
    title: str = Field(default="", description="Tiêu đề slide; với layout 'quote' là câu trích")
    subtitle: str = Field(default="", description="Phụ đề; với layout 'quote' là nguồn trích")
    bullets: list[str] = Field(default_factory=list, description="Nội dung cho layout 'bullets'")
    left: list[str] = Field(default_factory=list, description="Cột trái của 'two_column'")
    right: list[str] = Field(default_factory=list, description="Cột phải của 'two_column'")
    notes: str = Field(default="", description="Ghi chú người trình bày, không hiện khi chiếu")


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    database = app.database
    store: ArtifactStore = app.artifacts
    server = build_server(
        SERVER_NAME,
        "Private AI artifacts",
        INSTRUCTIONS,
        settings=app.settings,
    )

    async def _scope(workspace_id: str) -> str:
        """Validate the workspace before writing a folder named after it."""
        cleaned = workspace_id.strip()
        if not cleaned:
            return ""
        return await require_workspace(database, cleaned)

    def _written(path: Path, kind: str, title: str) -> dict[str, Any]:
        artifact = store.describe(path, kind, title)
        payload = artifact.public()
        payload["message"] = (
            f"Đã ghi {kind} vào {path}. Hãy báo đường dẫn này cho người dùng; "
            "ứng dụng không tự mở tệp."
        )
        return payload

    # --- charts -----------------------------------------------------------

    @server.tool(
        name="artifacts.create_chart",
        description=(
            "Vẽ biểu đồ tương tác thành một tệp HTML độc lập, mở được bằng trình duyệt "
            "và chạy hoàn toàn ngoại tuyến (di chuột đọc giá trị, cuộn để phóng to, kéo "
            "để trượt).\n\n"
            "chart_type: " + " · ".join(CHART_TYPES) + ".\n"
            "Dùng 'candlestick' cho biểu đồ giá — truyền 'candles' thay cho 'series'. "
            "Các loại còn lại dùng 'categories' (nhãn trục X) cùng 'series'; mỗi chuỗi "
            "phải có đúng số giá trị bằng số nhãn.\n"
            "value_format: "
            + " · ".join(VALUE_FORMATS)
            + ". Đặt 'unit' để hiện đơn vị.\n\n"
            + _SHARED_RULES
        ),
    )
    async def create_chart(
        title: str,
        chart_type: Literal[CHART_TYPES] = "line",  # type: ignore[valid-type]
        categories: list[str] = Field(  # noqa: B008 - pydantic builds the model, not Python
            default_factory=list, description="Nhãn trục X, một nhãn cho mỗi điểm dữ liệu"
        ),
        series: list[SeriesInput] = Field(default_factory=list),  # noqa: B008
        candles: list[CandleInput] = Field(default_factory=list),  # noqa: B008
        subtitle: str = "",
        x_label: str = "",
        y_label: str = "",
        unit: str = "",
        value_format: Literal[VALUE_FORMATS] = "number",  # type: ignore[valid-type]
        decimals: int = -1,
        source: str = "",
        notes: list[str] = Field(default_factory=list),  # noqa: B008
        workspace_id: str = "",
    ) -> dict[str, Any]:
        scope = await _scope(workspace_id)
        spec = ChartSpec(
            title=title,
            chart_type=chart_type,
            subtitle=subtitle,
            categories=list(categories),
            series=[ChartSeries(name=item.name, values=list(item.values)) for item in series],
            candles=[
                ChartCandle(
                    label=item.label,
                    open=item.open,
                    high=item.high,
                    low=item.low,
                    close=item.close,
                    volume=item.volume,
                )
                for item in candles
            ],
            x_label=x_label,
            y_label=y_label,
            unit=unit,
            value_format=value_format,
            decimals=decimals,
            source=source,
            notes=list(notes),
        )
        try:
            html = render_chart_page(spec)
            path = store.write_text(scope, title, ".html", html)
        except ArtifactError as exc:
            raise ToolError(str(exc)) from exc
        return _written(path, "chart", title)

    # --- diagrams ---------------------------------------------------------

    @server.tool(
        name="artifacts.create_diagram",
        description=(
            "Vẽ sơ đồ Mermaid thành một trang HTML, kèm tệp .mmd chứa mã nguồn. Dùng cho "
            "sơ đồ hệ thống, luồng xử lý, sơ đồ tuần tự, sơ đồ lớp, ERD, gantt hay "
            "mindmap.\n\n"
            "'source' là mã Mermaid, bắt đầu bằng khai báo loại sơ đồ ở dòng đầu — ví dụ "
            "'flowchart TB', 'sequenceDiagram', 'erDiagram'. Không bọc trong dấu ``` "
            "(nếu có thì sẽ được gỡ).\n\n"
            "Trang tải thư viện Mermaid từ CDN khi máy có mạng; khi ngoại tuyến trang vẫn "
            "mở được và hiển thị nguyên mã nguồn, nên hãy viết mã sao cho đọc được.\n\n"
            + _SHARED_RULES
        ),
    )
    async def create_diagram(
        title: str,
        source: str,
        subtitle: str = "",
        caption: str = "",
        notes: list[str] = Field(default_factory=list),  # noqa: B008
        workspace_id: str = "",
    ) -> dict[str, Any]:
        scope = await _scope(workspace_id)
        spec = DiagramSpec(
            title=title,
            source=source,
            subtitle=subtitle,
            caption=caption,
            notes=list(notes),
        )
        try:
            html = render_diagram_page(spec)
            path = store.write_text(scope, title, ".html", html)
            # The .mmd sits beside the page so the diagram survives without a browser.
            store.write_text(scope, title, ".mmd", spec.cleaned() + "\n")
        except ArtifactError as exc:
            raise ToolError(str(exc)) from exc
        return _written(path, "diagram", title)

    # --- office -----------------------------------------------------------

    @server.tool(
        name="artifacts.create_document",
        description=(
            "Soạn tệp Word (.docx) từ danh sách khối nội dung.\n\n"
            "Mỗi phần tử của 'blocks' có 'type' là một trong: " + " · ".join(BLOCK_TYPES) + ". "
            "heading dùng 'level' 1–4; paragraph/quote/code dùng 'text'; bullets/numbered "
            "dùng 'items'; table dùng 'rows' với dòng đầu là tiêu đề cột và mọi dòng cùng "
            "số ô (đặt 'text' cho chú thích bảng).\n\n"
            "Viết như một tài liệu thật: mở bằng một đoạn tóm tắt, rồi mới đến các mục. "
            + _SHARED_RULES
        ),
    )
    async def create_document(
        title: str,
        blocks: list[BlockInput],
        subtitle: str = "",
        author: str = "",
        workspace_id: str = "",
    ) -> dict[str, Any]:
        scope = await _scope(workspace_id)
        spec = DocumentSpec(
            title=title,
            subtitle=subtitle,
            author=author,
            blocks=[
                DocumentBlock(
                    type=block.type,
                    text=block.text,
                    level=block.level,
                    items=list(block.items),
                    rows=[list(row) for row in block.rows],
                )
                for block in blocks
            ],
        )
        try:
            payload = build_docx(spec)
            path = store.write_bytes(scope, title, ".docx", payload)
        except ArtifactError as exc:
            raise ToolError(str(exc)) from exc
        return _written(path, "document", title)

    @server.tool(
        name="artifacts.create_slides",
        description=(
            "Soạn bản trình chiếu PowerPoint (.pptx) khổ 16:9. Slide tiêu đề được thêm "
            "sẵn từ 'title'/'subtitle'; 'slides' là phần còn lại.\n\n"
            "layout mỗi slide: " + " · ".join(SLIDE_LAYOUTS) + ". "
            "'bullets' dùng trường bullets; 'two_column' dùng left và right; 'quote' đặt "
            "câu trích ở 'title' và nguồn ở 'subtitle'. Thụt đầu dòng hai dấu cách trong "
            "một mục để lùi một cấp.\n\n"
            "Mỗi slide một ý, tối đa khoảng sáu dòng; phần diễn giải dài đưa vào 'notes'. "
            + _SHARED_RULES
        ),
    )
    async def create_slides(
        title: str,
        slides: list[SlideInput],
        subtitle: str = "",
        author: str = "",
        workspace_id: str = "",
    ) -> dict[str, Any]:
        scope = await _scope(workspace_id)
        spec = SlidesSpec(
            title=title,
            subtitle=subtitle,
            author=author,
            slides=[
                SlideSpec(
                    layout=item.layout,
                    title=item.title,
                    subtitle=item.subtitle,
                    bullets=list(item.bullets),
                    left=list(item.left),
                    right=list(item.right),
                    notes=item.notes,
                )
                for item in slides
            ],
        )
        try:
            payload = build_pptx(spec)
            path = store.write_bytes(scope, title, ".pptx", payload)
        except ArtifactError as exc:
            raise ToolError(str(exc)) from exc
        return _written(path, "slides", title)

    # --- listing ----------------------------------------------------------

    @server.tool(
        name="artifacts.list",
        description=(
            "Liệt kê các tệp đã tạo trong workspace này, mới nhất trước. Dùng khi người "
            "dùng hỏi lại về một tệp vừa tạo hoặc muốn biết đường dẫn của nó."
        ),
    )
    async def list_artifacts(limit: int = 20, workspace_id: str = "") -> dict[str, Any]:
        scope = await _scope(workspace_id)
        found = store.listing(scope, max(1, min(limit, 100)))
        return {
            "folder": str(store.root / (scope or "chung")),
            "count": len(found),
            "artifacts": [item.public() for item in found],
        }

    return server


def run() -> None:
    stdio_entry(create_server)
