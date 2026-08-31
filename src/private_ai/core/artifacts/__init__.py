"""Files the agent produces: charts, diagrams, Word documents and decks.

The split is by output, not by tool: :mod:`store` decides where a file lands and under
what name, :mod:`page` holds the HTML shell the two web outputs share, and :mod:`chart`,
:mod:`diagram` and :mod:`office` each turn one validated spec into bytes. Nothing here
knows about MCP — the server in ``private_ai.mcp.servers.artifacts`` is a thin layer over
these functions, so a test can build a deck without standing a server up.

Validation lives on the specs rather than in the server for the same reason. A model
reads the error text and tries again, so "chuỗi 'Doanh thu' có 11 giá trị nhưng có 12
nhãn" is a working instruction and "invalid input" is a wasted turn.
"""

from __future__ import annotations

from private_ai.core.artifacts.chart import (
    CHART_TYPES,
    VALUE_FORMATS,
    ChartCandle,
    ChartSeries,
    ChartSpec,
    render_chart_page,
)
from private_ai.core.artifacts.diagram import (
    DIAGRAM_KEYWORDS,
    MERMAID_CDN,
    DiagramSpec,
    render_diagram_page,
)
from private_ai.core.artifacts.office import (
    BLOCK_TYPES,
    SLIDE_LAYOUTS,
    DocumentBlock,
    DocumentSpec,
    SlideSpec,
    SlidesSpec,
    build_docx,
    build_pptx,
)
from private_ai.core.artifacts.store import (
    Artifact,
    ArtifactError,
    ArtifactStore,
    slugify,
)

__all__ = [
    "BLOCK_TYPES",
    "CHART_TYPES",
    "DIAGRAM_KEYWORDS",
    "MERMAID_CDN",
    "SLIDE_LAYOUTS",
    "VALUE_FORMATS",
    "Artifact",
    "ArtifactError",
    "ArtifactStore",
    "ChartCandle",
    "ChartSeries",
    "ChartSpec",
    "DiagramSpec",
    "DocumentBlock",
    "DocumentSpec",
    "SlideSpec",
    "SlidesSpec",
    "build_docx",
    "build_pptx",
    "render_chart_page",
    "render_diagram_page",
    "slugify",
]
