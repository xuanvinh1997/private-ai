"""Word documents and PowerPoint decks.

``python-docx`` and ``python-pptx`` are imported inside the builders rather than at module
scope. Both pull in the whole OOXML layer, and the desktop process imports this package to
draw a chart far more often than it writes a deck; a missing one has to say so as tool
text a model can act on, not as an ImportError at startup.

The block and slide vocabularies are deliberately small. A model given thirty formatting
options spends its turn choosing between them; given eight, it spends the turn on the
content, which is the part it is actually good at.
"""

from __future__ import annotations

import io
from collections.abc import Sequence
from contextlib import suppress
from dataclasses import dataclass, field

from private_ai.core.artifacts.store import ArtifactError

__all__ = [
    "BLOCK_TYPES",
    "SLIDE_LAYOUTS",
    "DocumentBlock",
    "DocumentSpec",
    "SlideSpec",
    "SlidesSpec",
    "build_docx",
    "build_pptx",
]

BLOCK_TYPES = (
    "heading",
    "paragraph",
    "bullets",
    "numbered",
    "table",
    "quote",
    "code",
    "page_break",
)

SLIDE_LAYOUTS = ("title", "section", "bullets", "two_column", "quote", "blank")

MAX_HEADING_LEVEL = 4
MAX_TABLE_COLUMNS = 12
MAX_BULLET_DEPTH = 4


def _require(module: str, package: str):
    try:
        return __import__(module, fromlist=["*"])
    except ImportError as exc:  # pragma: no cover - depends on the install
        raise ArtifactError(
            f"Thiếu thư viện '{package}'. Cài bằng: uv pip install {package}"
        ) from exc


def _bullet_level(text: str) -> tuple[int, str]:
    """Leading indentation is the nesting level: two spaces or one tab per step."""
    stripped = text.lstrip("\t ")
    indent = len(text) - len(stripped)
    tabs = text[:indent].count("\t")
    spaces = indent - tabs
    level = min(MAX_BULLET_DEPTH - 1, tabs + spaces // 2)
    return level, stripped.removeprefix("- ").removeprefix("* ").strip()


# --- Word -----------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class DocumentBlock:
    """One thing on the page. ``type`` decides which of the other fields is read."""

    type: str
    text: str = ""
    level: int = 1
    items: Sequence[str] = field(default=())
    rows: Sequence[Sequence[str]] = field(default=())

    def validate(self, position: int) -> None:
        where = f"Khối #{position + 1} ({self.type})"
        if self.type not in BLOCK_TYPES:
            raise ArtifactError(
                f"{where}: loại không hỗ trợ. Chọn một trong: " + ", ".join(BLOCK_TYPES)
            )
        if self.type in {"heading", "paragraph", "quote", "code"} and not self.text.strip():
            raise ArtifactError(f"{where}: thiếu 'text'.")
        if self.type == "heading" and not 1 <= self.level <= MAX_HEADING_LEVEL:
            raise ArtifactError(f"{where}: 'level' phải từ 1 đến {MAX_HEADING_LEVEL}.")
        if self.type in {"bullets", "numbered"} and not any(item.strip() for item in self.items):
            raise ArtifactError(f"{where}: thiếu 'items'.")
        if self.type == "table":
            self._validate_table(where)

    def _validate_table(self, where: str) -> None:
        if len(self.rows) < 2:
            raise ArtifactError(f"{where}: bảng cần ít nhất hai dòng — dòng đầu là tiêu đề cột.")
        width = len(self.rows[0])
        if not width or width > MAX_TABLE_COLUMNS:
            raise ArtifactError(f"{where}: bảng phải có từ 1 đến {MAX_TABLE_COLUMNS} cột.")
        for index, row in enumerate(self.rows):
            if len(row) != width:
                raise ArtifactError(
                    f"{where}: dòng {index + 1} có {len(row)} ô nhưng tiêu đề có {width} cột."
                )


@dataclass(frozen=True, slots=True)
class DocumentSpec:
    title: str
    blocks: Sequence[DocumentBlock]
    subtitle: str = ""
    author: str = ""

    def validate(self) -> None:
        if not self.title.strip():
            raise ArtifactError("Thiếu 'title': tài liệu cần một tiêu đề.")
        if not self.blocks:
            raise ArtifactError("Thiếu 'blocks': tài liệu không có nội dung nào.")
        for position, block in enumerate(self.blocks):
            block.validate(position)


def _styled(paragraph, style: str) -> None:
    """Apply a built-in style, and carry on plainly if this template lacks it."""
    # pragma: no cover - only on a template without the default styles
    with suppress(KeyError):
        paragraph.style = style


def build_docx(spec: DocumentSpec) -> bytes:
    """A .docx as bytes, so the caller decides where it lands."""
    spec.validate()
    docx = _require("docx", "python-docx")
    shared = _require("docx.shared", "python-docx")

    document = docx.Document()
    if spec.author.strip():
        document.core_properties.author = spec.author.strip()
    document.core_properties.title = spec.title.strip()

    heading = document.add_paragraph(spec.title.strip())
    _styled(heading, "Title")
    if spec.subtitle.strip():
        subtitle = document.add_paragraph(spec.subtitle.strip())
        _styled(subtitle, "Subtitle")

    for block in spec.blocks:
        if block.type == "heading":
            document.add_heading(block.text.strip(), level=block.level)
        elif block.type == "paragraph":
            document.add_paragraph(block.text.strip())
        elif block.type == "quote":
            _styled(document.add_paragraph(block.text.strip()), "Intense Quote")
        elif block.type == "code":
            paragraph = document.add_paragraph()
            run = paragraph.add_run(block.text.rstrip())
            run.font.name = "Consolas"
            run.font.size = shared.Pt(10)
        elif block.type in {"bullets", "numbered"}:
            base = "List Bullet" if block.type == "bullets" else "List Number"
            for item in block.items:
                if not item.strip():
                    continue
                level, text = _bullet_level(item)
                # Word names the nested variants "List Bullet 2", "List Bullet 3", ...
                style = base if level == 0 else f"{base} {min(level, 2) + 1}"
                _styled(document.add_paragraph(text), style)
        elif block.type == "table":
            _add_table(document, block)
        elif block.type == "page_break":
            document.add_page_break()

    buffer = io.BytesIO()
    document.save(buffer)
    return buffer.getvalue()


def _add_table(document, block: DocumentBlock) -> None:
    header, *body = block.rows
    table = document.add_table(rows=1, cols=len(header))
    with suppress(KeyError):  # pragma: no cover - template dependent
        table.style = "Light Grid Accent 1"
    for cell, text in zip(table.rows[0].cells, header, strict=True):
        cell.text = str(text)
        for paragraph in cell.paragraphs:
            for run in paragraph.runs:
                run.bold = True
    for row in body:
        cells = table.add_row().cells
        for cell, text in zip(cells, row, strict=True):
            cell.text = "" if text is None else str(text)
    if block.text.strip():
        caption = document.add_paragraph(block.text.strip())
        _styled(caption, "Caption")


# --- PowerPoint -----------------------------------------------------------


@dataclass(frozen=True, slots=True)
class SlideSpec:
    layout: str = "bullets"
    title: str = ""
    subtitle: str = ""
    bullets: Sequence[str] = field(default=())
    left: Sequence[str] = field(default=())
    right: Sequence[str] = field(default=())
    notes: str = ""

    def validate(self, position: int) -> None:
        where = f"Slide #{position + 1}"
        if self.layout not in SLIDE_LAYOUTS:
            raise ArtifactError(
                f"{where}: layout '{self.layout}' không hỗ trợ. Chọn: " + ", ".join(SLIDE_LAYOUTS)
            )
        if self.layout != "blank" and not self.title.strip():
            raise ArtifactError(f"{where}: thiếu 'title'.")
        if self.layout == "bullets" and not any(item.strip() for item in self.bullets):
            raise ArtifactError(f"{where}: layout 'bullets' cần 'bullets'.")
        if self.layout == "two_column" and not (self.left and self.right):
            raise ArtifactError(f"{where}: layout 'two_column' cần cả 'left' và 'right'.")
        if self.layout == "quote" and not self.subtitle.strip():
            raise ArtifactError(
                f"{where}: layout 'quote' đặt câu trích trong 'title' và nguồn trong 'subtitle'."
            )


@dataclass(frozen=True, slots=True)
class SlidesSpec:
    title: str
    slides: Sequence[SlideSpec]
    subtitle: str = ""
    author: str = ""

    def validate(self) -> None:
        if not self.title.strip():
            raise ArtifactError("Thiếu 'title': bản trình chiếu cần một tiêu đề.")
        if not self.slides:
            raise ArtifactError("Thiếu 'slides': bản trình chiếu không có slide nào.")
        for position, slide in enumerate(self.slides):
            slide.validate(position)


# Indices into the default python-pptx template. Named here so the intent survives even
# though the numbers themselves are the template's, not ours.
_LAYOUT_TITLE = 0
_LAYOUT_CONTENT = 1
_LAYOUT_SECTION = 2
_LAYOUT_TWO_CONTENT = 3
_LAYOUT_TITLE_ONLY = 5
_LAYOUT_BLANK = 6


def _fill_bullets(frame, items: Sequence[str]) -> None:
    frame.clear()
    frame.word_wrap = True
    first = True
    for item in items:
        if not item.strip():
            continue
        level, text = _bullet_level(item)
        paragraph = frame.paragraphs[0] if first else frame.add_paragraph()
        paragraph.text = text
        paragraph.level = level
        first = False


def build_pptx(spec: SlidesSpec) -> bytes:
    """A 16:9 deck as bytes."""
    spec.validate()
    pptx = _require("pptx", "python-pptx")
    util = _require("pptx.util", "python-pptx")

    presentation = pptx.Presentation()
    # The stock template is 4:3. Every screen a deck is shown on is not.
    presentation.slide_width = util.Inches(13.333)
    presentation.slide_height = util.Inches(7.5)
    presentation.core_properties.title = spec.title.strip()
    if spec.author.strip():
        presentation.core_properties.author = spec.author.strip()

    layouts = presentation.slide_layouts
    cover = presentation.slides.add_slide(layouts[_LAYOUT_TITLE])
    cover.shapes.title.text = spec.title.strip()
    if len(cover.placeholders) > 1:
        cover.placeholders[1].text = spec.subtitle.strip() or spec.author.strip()

    for slide_spec in spec.slides:
        slide = _add_slide(presentation, layouts, slide_spec, util)
        if slide_spec.notes.strip():
            slide.notes_slide.notes_text_frame.text = slide_spec.notes.strip()

    buffer = io.BytesIO()
    presentation.save(buffer)
    return buffer.getvalue()


def _add_slide(presentation, layouts, spec: SlideSpec, util):
    if spec.layout == "blank":
        return presentation.slides.add_slide(layouts[_LAYOUT_BLANK])

    if spec.layout == "section":
        slide = presentation.slides.add_slide(layouts[_LAYOUT_SECTION])
        slide.shapes.title.text = spec.title.strip()
        if spec.subtitle.strip() and len(slide.placeholders) > 1:
            slide.placeholders[1].text = spec.subtitle.strip()
        return slide

    if spec.layout == "title":
        slide = presentation.slides.add_slide(layouts[_LAYOUT_TITLE])
        slide.shapes.title.text = spec.title.strip()
        if len(slide.placeholders) > 1:
            slide.placeholders[1].text = spec.subtitle.strip()
        return slide

    if spec.layout == "quote":
        slide = presentation.slides.add_slide(layouts[_LAYOUT_TITLE_ONLY])
        title = slide.shapes.title
        title.text = spec.title.strip()
        title.text_frame.word_wrap = True
        for paragraph in title.text_frame.paragraphs:
            for run in paragraph.runs:
                run.font.size = util.Pt(30)
                run.font.italic = True
        box = slide.shapes.add_textbox(
            util.Inches(1.0), util.Inches(4.6), util.Inches(11.3), util.Inches(0.9)
        )
        run = box.text_frame.paragraphs[0].add_run()
        run.text = "— " + spec.subtitle.strip()
        run.font.size = util.Pt(16)
        return slide

    if spec.layout == "two_column":
        slide = presentation.slides.add_slide(layouts[_LAYOUT_TWO_CONTENT])
        slide.shapes.title.text = spec.title.strip()
        bodies = [
            placeholder
            for placeholder in slide.placeholders
            if placeholder.placeholder_format.idx != 0
        ]
        if len(bodies) >= 2:
            _fill_bullets(bodies[0].text_frame, spec.left)
            _fill_bullets(bodies[1].text_frame, spec.right)
        return slide

    slide = presentation.slides.add_slide(layouts[_LAYOUT_CONTENT])
    slide.shapes.title.text = spec.title.strip()
    body = next(
        (p for p in slide.placeholders if p.placeholder_format.idx != 0),
        None,
    )
    if body is not None:
        _fill_bullets(body.text_frame, spec.bullets)
    return slide
