"""The knowledge graph, drawn on QGraphicsScene with a force simulation of our own.

Ported from a Cytoscape.js ``cose`` layout. Qt has no force layout, so this file carries
one: a Fruchterman–Reingold step on numpy arrays, driven by a ``QTimer`` and cooled to a
stop. At the 500-node ceiling the all-pairs repulsion is a 500×500 matrix per tick, which
numpy does in well under a frame.

Three behaviours from the old view are load-bearing and are kept exactly:

* The graph **accumulates**. Expanding a node merges a new slice into what is already
  drawn; only changing the focus, depth or limit rebuilds. A ``generation`` counter makes
  a slice that arrives after the base changed get dropped instead of merged into the
  wrong graph.
* Expansion places the new neighbours on a **circle around their parent** rather than
  re-running the layout, so nothing the user has already read jumps somewhere else.
* Sizes and colours are **derived, not stored**: degree decides size after every merge,
  and an entity type always hashes to the same palette slot, so the legend and the nodes
  cannot disagree.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

import numpy as np
from PySide6.QtCore import QStringListModel, Qt, QTimer
from PySide6.QtGui import QBrush, QColor, QFont, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import (
    QComboBox,
    QCompleter,
    QFrame,
    QGraphicsEllipseItem,
    QGraphicsItem,
    QGraphicsLineItem,
    QGraphicsScene,
    QGraphicsSimpleTextItem,
    QGraphicsView,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QScrollArea,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import theme
from private_ai.ui.icons import icon

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.ui.context import AppContext

# The eight-entry palette the web app used. An entity type is hashed into it, so the same
# type is the same colour across sessions, workspaces and the legend.
PALETTE = (
    "#1c7a63",
    "#3d6fb4",
    "#a8672c",
    "#7d55ab",
    "#a8465c",
    "#2f8f8a",
    "#5c6f3a",
    "#8a5a86",
)

# Expanding asks only for direct neighbours: enough to add one layer without dragging the
# whole store back over.
EXPAND_DEPTH = 1
EXPAND_LIMIT = 40

DEPTHS = (1, 2, 3, 4)
LIMITS = (50, 150, 300, 500)

SUGGEST_DEBOUNCE_MS = 220
TICK_MS = 32

# Layout constants: `K` is the ideal edge length, everything else is scaled off it.
K = 90.0
COOLING = 0.94
INITIAL_TEMPERATURE = 220.0
MINIMUM_TEMPERATURE = 0.8
CENTER_PULL = 0.006
MIN_ZOOM = 0.15
MAX_ZOOM = 3.2
FADED_OPACITY = 0.12


def hash_of(text: str) -> int:
    """The web app's string hash, reproduced so colours and angles do not shift."""
    value = 0
    for char in text:
        value = (value * 31 + ord(char)) & 0xFFFFFFFF
    return value


def color_of(entity_type: str) -> str:
    return PALETTE[hash_of(entity_type) % len(PALETTE)]


def size_of(degree: int) -> float:
    return 18.0 + min(30.0, math.sqrt(degree) * 9.0)


def angle_seed(identifier: str) -> float:
    """Each node fans its new layer from an angle of its own, so two neighbouring parents
    do not stack their rings on top of each other."""
    return math.radians(hash_of(identifier) % 360)


def edge_key(edge: dict[str, Any]) -> str:
    """Relations are undirected, so the ends are sorted: loading twice must not make two."""
    ends = sorted([str(edge.get("source", "")), str(edge.get("target", ""))])
    return f"{ends[0]}|{ends[1]}|{edge.get('type') or ''}"


def _text(properties: Any, key: str) -> str:
    if not isinstance(properties, dict):
        return ""
    value = properties.get(key)
    return value if isinstance(value, str) else ""


def _number(properties: Any, key: str) -> float:
    if not isinstance(properties, dict):
        return 0.0
    value = properties.get(key)
    return float(value) if isinstance(value, int | float) and math.isfinite(value) else 0.0


@dataclass(slots=True)
class GraphNode:
    id: str
    label: str
    type: str
    description: str = ""
    file: str = ""
    degree: int = 0
    layer: int = 0
    x: float = 0.0
    y: float = 0.0
    pinned: bool = False
    expanded: bool = False

    @property
    def color(self) -> str:
        return color_of(self.type)

    @property
    def size(self) -> float:
        return size_of(self.degree)


@dataclass(slots=True)
class GraphEdge:
    key: str
    source: str
    target: str
    label: str
    description: str = ""
    weight: float = 0.0
    file: str = ""


@dataclass(slots=True)
class _Selection:
    kind: str  # "node" | "edge"
    node: GraphNode | None = None
    edge: GraphEdge | None = None
    ends: tuple[GraphNode, GraphNode] | None = None


def _swatch_icon(color: QColor) -> QIcon:
    pixmap = QPixmap(12, 12)
    pixmap.fill(color)
    return QIcon(pixmap)


# --- scene items ----------------------------------------------------------


class _NodeItem(QGraphicsEllipseItem):
    def __init__(self, view: GraphView, node: GraphNode) -> None:
        radius = node.size / 2
        super().__init__(-radius, -radius, node.size, node.size)
        self._view = view
        self.node = node
        self.setZValue(2)
        self.setAcceptHoverEvents(True)
        self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemIsMovable, True)
        self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemIsSelectable, True)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.label = QGraphicsSimpleTextItem(node.label, self)
        self.label.setZValue(3)
        font = QFont()
        font.setPointSizeF(7.5)
        font.setBold(True)
        self.label.setFont(font)
        self.apply_theme()

    def apply_theme(self) -> None:
        tokens = theme.tokens()
        self.setBrush(QBrush(QColor(self.node.color)))
        self.setPen(QPen(QColor(tokens.get("surface", "#ffffff")), 2))
        self.label.setBrush(QBrush(QColor(tokens.get("text", "#293732"))))
        self.reposition_label()

    def resize(self) -> None:
        radius = self.node.size / 2
        self.setRect(-radius, -radius, self.node.size, self.node.size)
        self.reposition_label()

    def reposition_label(self) -> None:
        bounds = self.label.boundingRect()
        self.label.setPos(-bounds.width() / 2, self.node.size / 2 + 3)

    def set_selected_ring(self, selected: bool) -> None:
        tokens = theme.tokens()
        if selected:
            self.setPen(QPen(QColor(tokens.get("ink", "#17231f")), 3))
        elif self.node.expanded:
            self.setPen(QPen(QColor(tokens.get("accent", "#176b59")), 3))
        else:
            self.setPen(QPen(QColor(tokens.get("surface", "#ffffff")), 2))

    # --- interaction ------------------------------------------------------

    def hoverEnterEvent(self, event) -> None:  # noqa: N802
        self._view.spotlight_node(self.node.id)
        super().hoverEnterEvent(event)

    def hoverLeaveEvent(self, event) -> None:  # noqa: N802
        self._view.clear_spotlight()
        super().hoverLeaveEvent(event)

    def mousePressEvent(self, event) -> None:  # noqa: N802
        # Dragging is a deliberate rearrangement; a leftover fade would only get in the way.
        self.node.pinned = True
        self._view.clear_spotlight()
        self._view.select_node(self.node)
        super().mousePressEvent(event)

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802
        self.node.pinned = False
        self._view.commit_position(self)
        super().mouseReleaseEvent(event)

    def mouseDoubleClickEvent(self, event) -> None:  # noqa: N802
        self._view.expand_node(self.node.id)
        event.accept()


class _EdgeItem(QGraphicsLineItem):
    def __init__(self, view: GraphView, edge: GraphEdge) -> None:
        super().__init__()
        self._view = view
        self.edge = edge
        self.setZValue(1)
        self.setAcceptHoverEvents(True)
        self.label = QGraphicsSimpleTextItem(edge.label)
        self.label.setZValue(4)
        font = QFont()
        font.setPointSizeF(7.0)
        font.setBold(True)
        self.label.setFont(font)
        self.label.setVisible(False)
        self.apply_theme()

    def apply_theme(self) -> None:
        tokens = theme.tokens()
        self._idle = QPen(QColor(tokens.get("line-strong", "#c2cec8")), 1.4)
        self._lit = QPen(QColor(tokens.get("accent", "#176b59")), 2.8)
        self.setPen(self._idle)
        self.label.setBrush(QBrush(QColor(tokens.get("accent-ink", "#0c4d3f"))))

    def set_lit(self, lit: bool) -> None:
        self.setPen(self._lit if lit else self._idle)
        self.label.setVisible(lit)
        if lit:
            line = self.line()
            self.label.setPos(
                (line.x1() + line.x2()) / 2,
                (line.y1() + line.y2()) / 2,
            )

    def hoverEnterEvent(self, event) -> None:  # noqa: N802
        self._view.spotlight_edge(self.edge.key)
        super().hoverEnterEvent(event)

    def hoverLeaveEvent(self, event) -> None:  # noqa: N802
        self._view.clear_spotlight()
        super().hoverLeaveEvent(event)

    def mousePressEvent(self, event) -> None:  # noqa: N802
        self._view.select_edge(self.edge)
        event.accept()


class _Canvas(QGraphicsView):
    """Wheel zooms, empty-space drag pans, node drag moves the node."""

    def __init__(self, scene: QGraphicsScene, parent=None) -> None:
        super().__init__(scene, parent)
        self.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        self.setTransformationAnchor(QGraphicsView.ViewportAnchor.AnchorUnderMouse)
        self.setDragMode(QGraphicsView.DragMode.NoDrag)
        self.setFrameShape(QFrame.Shape.NoFrame)
        self._on_background_click = None

    def set_background_handler(self, handler) -> None:
        self._on_background_click = handler

    def wheelEvent(self, event) -> None:  # noqa: N802
        factor = 1.15 ** (event.angleDelta().y() / 120.0)
        self.zoom_by(factor)
        event.accept()

    def zoom_by(self, factor: float) -> None:
        current = self.transform().m11()
        target = max(MIN_ZOOM, min(MAX_ZOOM, current * factor))
        if target > 0 and current > 0:
            self.scale(target / current, target / current)

    def mousePressEvent(self, event) -> None:  # noqa: N802
        if self.itemAt(event.position().toPoint()) is None:
            self.setDragMode(QGraphicsView.DragMode.ScrollHandDrag)
            if self._on_background_click is not None:
                self._on_background_click()
        super().mousePressEvent(event)

    def mouseReleaseEvent(self, event) -> None:  # noqa: N802
        super().mouseReleaseEvent(event)
        self.setDragMode(QGraphicsView.DragMode.NoDrag)


# --- the view -------------------------------------------------------------


class GraphView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx

        # Accumulated graph. `_generation` invalidates in-flight expansions.
        self._nodes: dict[str, GraphNode] = {}
        self._edges: dict[str, GraphEdge] = {}
        self._dangling: dict[str, dict] = {}
        self._node_items: dict[str, _NodeItem] = {}
        self._edge_items: dict[str, _EdgeItem] = {}
        self._generation = 0
        self._layers = 0
        self._expanding = ""
        self._muted: set[str] = set()
        self._selection: _Selection | None = None
        self._truncated = False
        self._loading = False

        self._focus = "*"
        self._depth = 2
        self._limit = 150

        # Simulation state, kept parallel to `_order`.
        self._order: list[str] = []
        self._positions = np.zeros((0, 2), dtype=np.float64)
        self._edge_source = np.zeros(0, dtype=np.int64)
        self._edge_target = np.zeros(0, dtype=np.int64)
        self._temperature = 0.0

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 20, 24, 20)
        root.setSpacing(10)
        self._build_heading(root)
        self._build_toolbar(root)

        body = QHBoxLayout()
        body.setSpacing(12)
        body.addWidget(self._build_stage(), 1)
        body.addWidget(self._build_side(), 0)
        root.addLayout(body, 1)

        self._timer = QTimer(self)
        self._timer.setInterval(TICK_MS)
        self._timer.timeout.connect(self._tick)

        self._suggest_timer = QTimer(self)
        self._suggest_timer.setSingleShot(True)
        self._suggest_timer.setInterval(SUGGEST_DEBOUNCE_MS)
        self._suggest_timer.timeout.connect(self._fetch_suggestions)

        ctx.themeChanged.connect(self._on_theme)
        ctx.workspaceChanged.connect(self._on_workspace)
        ctx.documentsChanged.connect(self.reload)
        self._on_theme(ctx.theme_name)
        self.reload()

    # --- construction -----------------------------------------------------

    def _build_heading(self, root: QVBoxLayout) -> None:
        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Kho tri thức")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        title = QLabel("Đồ thị tri thức")
        title.setProperty("class", "title")
        titles.addWidget(title)
        blurb = QLabel(
            "Thực thể và quan hệ mà Private AI rút ra từ tài liệu. Kéo node để sắp lại, kéo "
            "nền để dời khung, lăn chuột để phóng to, bấm một node hoặc một đường nối để xem "
            "chi tiết. Bấm đúp vào một node để mở thêm một lớp lân cận ngay trên hình đang có."
        )
        blurb.setWordWrap(True)
        blurb.setProperty("class", "muted")
        titles.addWidget(blurb)
        heading.addLayout(titles, 1)

        self._whole_graph = QPushButton("Toàn bộ đồ thị")
        self._whole_graph.clicked.connect(lambda: self.focus_entity("*"))
        self._whole_graph.hide()
        heading.addWidget(self._whole_graph, 0, Qt.AlignmentFlag.AlignTop)

        self._relayout = QPushButton("Sắp xếp lại")
        self._relayout.clicked.connect(self.relayout)
        self._relayout.hide()
        heading.addWidget(self._relayout, 0, Qt.AlignmentFlag.AlignTop)

        reload_button = QPushButton("Tải lại")
        reload_button.setIcon(icon("refresh-cw"))
        reload_button.clicked.connect(self.reload)
        heading.addWidget(reload_button, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

    def _build_toolbar(self, root: QVBoxLayout) -> None:
        toolbar = QHBoxLayout()
        self._search = QLineEdit()
        self._search.setClearButtonEnabled(True)
        self._search.setPlaceholderText("Tìm thực thể để xem lân cận")
        self._search.addAction(icon("search"), QLineEdit.ActionPosition.LeadingPosition)
        self._suggestions = QStringListModel([], self)
        self._completer = QCompleter(self._suggestions, self)
        self._completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self._completer.setFilterMode(Qt.MatchFlag.MatchContains)
        self._completer.activated.connect(self.focus_entity)
        self._search.setCompleter(self._completer)
        self._search.textEdited.connect(lambda _: self._suggest_timer.start())
        self._search.returnPressed.connect(
            lambda: self.focus_entity(self._search.text().strip() or "*")
        )
        toolbar.addWidget(self._search, 1)

        toolbar.addWidget(QLabel("Độ sâu"))
        self._depth_box = QComboBox()
        for value in DEPTHS:
            self._depth_box.addItem(str(value), value)
        self._depth_box.setCurrentIndex(DEPTHS.index(self._depth))
        self._depth_box.currentIndexChanged.connect(self._on_depth)
        toolbar.addWidget(self._depth_box)

        toolbar.addWidget(QLabel("Số node"))
        self._limit_box = QComboBox()
        for value in LIMITS:
            self._limit_box.addItem(str(value), value)
        self._limit_box.setCurrentIndex(LIMITS.index(self._limit))
        self._limit_box.currentIndexChanged.connect(self._on_limit)
        toolbar.addWidget(self._limit_box)

        # No zoom glyphs in the shared icon set, so these carry their own label.
        for label, tip, handler in (
            ("−", "Thu nhỏ", lambda: self._canvas.zoom_by(1 / 1.2)),
            ("+", "Phóng to", lambda: self._canvas.zoom_by(1.2)),
        ):
            button = QToolButton()
            button.setText(label)
            button.setToolTip(tip)
            button.clicked.connect(handler)
            toolbar.addWidget(button)
        fit_button = QToolButton()
        fit_button.setIcon(icon("eye"))
        fit_button.setToolTip("Vừa khung hình")
        fit_button.clicked.connect(self.fit)
        toolbar.addWidget(fit_button)
        root.addLayout(toolbar)

    def _build_stage(self) -> QWidget:
        container = QWidget()
        layout = QVBoxLayout(container)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)

        self._scene = QGraphicsScene(self)
        self._canvas = _Canvas(self._scene, container)
        self._canvas.set_background_handler(self._clear_selection)
        layout.addWidget(self._canvas, 1)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        layout.addWidget(self._empty)

        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "faint")
        layout.addWidget(self._status)

        self._warning = QLabel("")
        self._warning.setWordWrap(True)
        self._warning.setProperty("class", "danger")
        self._warning.hide()
        layout.addWidget(self._warning)
        return container

    def _build_side(self) -> QWidget:
        side = QWidget()
        side.setFixedWidth(280)
        layout = QVBoxLayout(side)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(10)

        legend_title = QLabel("Loại thực thể")
        legend_title.setProperty("class", "section-label")
        layout.addWidget(legend_title)
        self._legend_title = legend_title

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setMaximumHeight(220)
        legend_host = QWidget()
        self._legend = QVBoxLayout(legend_host)
        self._legend.setSpacing(3)
        self._legend.setContentsMargins(0, 0, 0, 0)
        self._legend.addStretch(1)
        scroll.setWidget(legend_host)
        layout.addWidget(scroll)

        self._detail = QFrame()
        self._detail.setProperty("class", "card")
        self._detail_layout = QVBoxLayout(self._detail)
        self._detail_layout.setSpacing(6)
        layout.addWidget(self._detail, 1)
        self._render_detail()
        return side

    # --- lifecycle --------------------------------------------------------

    def on_activated(self) -> None:
        if not self._nodes:
            self.reload()

    def on_deactivated(self) -> None:
        self._timer.stop()

    def _on_workspace(self, _workspace_id: str) -> None:
        self._focus = "*"
        self._search.clear()
        self.reload()

    def _on_theme(self, _name: str) -> None:
        tokens = theme.tokens()
        self._scene.setBackgroundBrush(QBrush(QColor(tokens.get("surface", "#ffffff"))))
        for item in self._node_items.values():
            item.apply_theme()
        for item in self._edge_items.values():
            item.apply_theme()
        self._sync_selection_rings()
        self._render_legend()

    # --- data -------------------------------------------------------------

    def reload(self) -> None:
        workspace_id = self._ctx.workspace_id
        if not workspace_id:
            self._reset_scene()
            self._show_empty("Hãy mở một không gian làm việc để xem đồ thị của nó.")
            return
        if self._loading:
            return
        self._loading = True
        self._show_empty("Đang dựng đồ thị…")
        self._ctx.run(
            self._ctx.services.graph.knowledge_graph(
                workspace_id, self._focus, self._depth, self._limit
            ),
            on_result=self._render_base,
            on_error=self._failed,
        )

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._show_empty(str(exc) or "Không đọc được đồ thị tri thức.")

    def focus_entity(self, name: str) -> None:
        self._focus = name or "*"
        self._search.setText("" if self._focus == "*" else self._focus)
        self._whole_graph.setVisible(self._focus != "*")
        self._depth_box.setEnabled(self._focus != "*")
        self.reload()

    def _on_depth(self) -> None:
        self._depth = int(self._depth_box.currentData())
        self.reload()

    def _on_limit(self) -> None:
        self._limit = int(self._limit_box.currentData())
        self.reload()

    def _fetch_suggestions(self) -> None:
        workspace_id = self._ctx.workspace_id
        if not workspace_id:
            return
        self._ctx.run(
            self._ctx.services.graph.find_entities(self._search.text().strip(), workspace_id, 40),
            on_result=self._show_suggestions,
            on_error=lambda _: None,
        )

    def _show_suggestions(self, entities: list[dict]) -> None:
        self._suggestions.setStringList(
            [str(item.get("name") or "") for item in entities if item.get("name")]
        )

    # --- merging ----------------------------------------------------------

    def _ingest(self, payload: dict, layer: int) -> tuple[list[GraphNode], list[GraphEdge]]:
        """Merge one slice into what is already held; report only what is genuinely new."""
        fresh_nodes: list[GraphNode] = []
        for raw in payload.get("nodes") or []:
            identifier = str(raw.get("id") or "")
            if not identifier or identifier in self._nodes:
                continue
            properties = raw.get("properties") or {}
            labels = raw.get("labels") or []
            node = GraphNode(
                id=identifier,
                label=str(labels[0]) if labels else identifier,
                type=_text(properties, "entity_type") or "khác",
                description=_text(properties, "description"),
                file=_text(properties, "file_path"),
                layer=layer,
            )
            self._nodes[identifier] = node
            fresh_nodes.append(node)

        for raw in payload.get("edges") or []:
            if raw.get("source") == raw.get("target"):
                continue
            key = edge_key(raw)
            if key not in self._edges:
                self._dangling.setdefault(key, raw)

        # An edge only draws once both ends exist, so a new layer can revive an edge that
        # was dangling when the previous one arrived. That is why unusable edges are kept
        # rather than dropped.
        fresh_edges: list[GraphEdge] = []
        for key in list(self._dangling):
            raw = self._dangling[key]
            source = str(raw.get("source") or "")
            target = str(raw.get("target") or "")
            if source not in self._nodes or target not in self._nodes:
                continue
            properties = raw.get("properties") or {}
            edge = GraphEdge(
                key=key,
                source=source,
                target=target,
                label=_text(properties, "keywords") or str(raw.get("type") or "") or "liên quan",
                description=_text(properties, "description"),
                weight=_number(properties, "weight"),
                file=_text(properties, "file_path"),
            )
            self._edges[key] = edge
            del self._dangling[key]
            fresh_edges.append(edge)
        return fresh_nodes, fresh_edges

    def _reset_scene(self) -> None:
        self._timer.stop()
        self._scene.clear()
        self._nodes.clear()
        self._edges.clear()
        self._dangling.clear()
        self._node_items.clear()
        self._edge_items.clear()
        self._order = []
        self._positions = np.zeros((0, 2), dtype=np.float64)
        self._edge_source = np.zeros(0, dtype=np.int64)
        self._edge_target = np.zeros(0, dtype=np.int64)
        self._selection = None
        self._layers = 0
        self._render_detail()
        self._render_legend()

    def _render_base(self, payload: dict) -> None:
        """A new base slice discards everything accumulated: the user changed the frame."""
        self._loading = False
        self._generation += 1
        self._reset_scene()
        self._truncated = bool(payload.get("truncated"))
        nodes, edges = self._ingest(payload, 0)
        if not nodes:
            self._show_empty(
                "Chưa có thực thể nào trong không gian này. Tải tài liệu lên rồi chờ lập chỉ "
                "mục xong, đồ thị sẽ tự có dữ liệu."
            )
            return
        self._empty.hide()
        self._canvas.show()
        # A ring of random-ish starting points beats a single pile: the simulation only has
        # to separate them, not to unstack them.
        count = len(nodes)
        for index, node in enumerate(nodes):
            angle = 2 * math.pi * index / max(1, count)
            radius = 60 + 30 * math.sqrt(count) * (0.4 + (hash_of(node.id) % 100) / 160)
            node.x = radius * math.cos(angle)
            node.y = radius * math.sin(angle)
        self._add_items(nodes, edges)
        self._layers = 1
        self._refresh_stats()
        self.relayout()

    def _add_items(self, nodes: list[GraphNode], edges: list[GraphEdge]) -> None:
        for node in nodes:
            item = _NodeItem(self, node)
            item.setPos(node.x, node.y)
            item.setVisible(node.type not in self._muted)
            self._scene.addItem(item)
            self._node_items[node.id] = item
        for edge in edges:
            item = _EdgeItem(self, edge)
            self._scene.addItem(item)
            self._scene.addItem(item.label)
            self._edge_items[edge.key] = item
        self._rebuild_arrays()
        self._sync_edge_visibility()
        self._draw()

    def _rebuild_arrays(self) -> None:
        self._order = list(self._nodes)
        self._positions = np.array(
            [[self._nodes[key].x, self._nodes[key].y] for key in self._order],
            dtype=np.float64,
        ).reshape(-1, 2)
        index = {key: position for position, key in enumerate(self._order)}
        pairs = [
            (index[edge.source], index[edge.target])
            for edge in self._edges.values()
            if edge.source in index and edge.target in index
        ]
        self._edge_source = np.array([p[0] for p in pairs], dtype=np.int64)
        self._edge_target = np.array([p[1] for p in pairs], dtype=np.int64)

    def _refresh_stats(self) -> None:
        """Degree, and therefore size, is read off the drawn graph — every new layer
        changes how many relations an already-placed node has."""
        degrees: dict[str, int] = dict.fromkeys(self._nodes, 0)
        for edge in self._edges.values():
            if edge.source in degrees:
                degrees[edge.source] += 1
            if edge.target in degrees:
                degrees[edge.target] += 1
        for identifier, degree in degrees.items():
            node = self._nodes[identifier]
            if node.degree != degree:
                node.degree = degree
                item = self._node_items.get(identifier)
                if item is not None:
                    item.resize()
        self._render_legend()
        self._render_status()

    # --- simulation -------------------------------------------------------

    def relayout(self) -> None:
        if not self._order:
            return
        self._temperature = INITIAL_TEMPERATURE
        self._timer.start()
        self._render_status()

    def _tick(self) -> None:
        if self._positions.shape[0] < 2 or self._temperature <= MINIMUM_TEMPERATURE:
            self._timer.stop()
            self._render_status()
            self.fit()
            return
        # A node the user is holding is ground truth, not a simulated body.
        for index, key in enumerate(self._order):
            node = self._nodes[key]
            if node.pinned:
                item = self._node_items.get(key)
                if item is not None:
                    self._positions[index] = (item.pos().x(), item.pos().y())

        points = self._positions
        delta = points[:, None, :] - points[None, :, :]
        distance = np.sqrt((delta**2).sum(axis=2))
        np.fill_diagonal(distance, np.inf)
        distance = np.maximum(distance, 0.01)
        repulsion = (K * K) / distance
        displacement = (delta / distance[:, :, None] * repulsion[:, :, None]).sum(axis=1)

        if self._edge_source.size:
            edge_delta = points[self._edge_source] - points[self._edge_target]
            edge_distance = np.maximum(np.sqrt((edge_delta**2).sum(axis=1)), 0.01)
            attraction = (edge_distance**2) / K
            pull = edge_delta / edge_distance[:, None] * attraction[:, None]
            np.add.at(displacement, self._edge_source, -pull)
            np.add.at(displacement, self._edge_target, pull)

        # A gentle pull to the origin, or disconnected components drift apart forever.
        displacement -= points * CENTER_PULL * K

        magnitude = np.maximum(np.sqrt((displacement**2).sum(axis=1)), 1e-9)
        step = np.minimum(magnitude, self._temperature)
        moved = displacement / magnitude[:, None] * step[:, None]
        frozen = np.array([self._nodes[key].pinned for key in self._order], dtype=bool).reshape(
            -1, 1
        )
        self._positions = points + np.where(frozen, 0.0, moved)
        self._temperature *= COOLING
        self._draw()

    def _draw(self) -> None:
        for index, key in enumerate(self._order):
            node = self._nodes[key]
            node.x = float(self._positions[index, 0])
            node.y = float(self._positions[index, 1])
            item = self._node_items.get(key)
            if item is not None and not node.pinned:
                item.setPos(node.x, node.y)
        for key, item in self._edge_items.items():
            edge = self._edges.get(key)
            if edge is None:
                continue
            source = self._node_items.get(edge.source)
            target = self._node_items.get(edge.target)
            if source is None or target is None:
                continue
            item.setLine(source.pos().x(), source.pos().y(), target.pos().x(), target.pos().y())

    def commit_position(self, item: _NodeItem) -> None:
        index = self._order.index(item.node.id) if item.node.id in self._order else -1
        if index >= 0:
            self._positions[index] = (item.pos().x(), item.pos().y())
        self._draw()

    def fit(self) -> None:
        bounds = self._scene.itemsBoundingRect()
        if bounds.isEmpty():
            return
        self._canvas.fitInView(
            bounds.adjusted(-40, -40, 40, 40), Qt.AspectRatioMode.KeepAspectRatio
        )
        scale = self._canvas.transform().m11()
        if scale > MAX_ZOOM:
            self._canvas.zoom_by(MAX_ZOOM / scale)

    # --- expansion --------------------------------------------------------

    def expand_node(self, identifier: str) -> None:
        workspace_id = self._ctx.workspace_id
        node = self._nodes.get(identifier)
        if not workspace_id or node is None or self._expanding or node.expanded:
            return
        era = self._generation
        self._expanding = identifier
        self._warning.hide()
        self._render_status()
        self._ctx.run(
            self._ctx.services.graph.knowledge_graph(
                workspace_id, identifier, EXPAND_DEPTH, EXPAND_LIMIT
            ),
            on_result=lambda payload: self._merge_expansion(payload, identifier, era),
            on_error=lambda exc: self._expansion_failed(exc, era),
        )

    def _merge_expansion(self, payload: dict, identifier: str, era: int) -> None:
        # The base changed while this was in flight; merging it now would mix two graphs.
        if era != self._generation:
            return
        self._expanding = ""
        parent = self._nodes.get(identifier)
        parent_item = self._node_items.get(identifier)
        if parent is None or parent_item is None:
            return
        layer = self._layers
        nodes, edges = self._ingest(payload, layer)

        # Place the new layer on a ring around the parent instead of re-running the layout,
        # so everything already on screen stays exactly where the user left it.
        origin = parent_item.pos()
        radius = 110 + min(160, len(nodes) * 7)
        start = angle_seed(identifier)
        for index, node in enumerate(nodes):
            angle = start + (2 * math.pi * index) / max(1, len(nodes))
            node.x = origin.x() + radius * math.cos(angle)
            node.y = origin.y() + radius * math.sin(angle)
        self._add_items(nodes, edges)
        self._refresh_stats()

        parent.expanded = True
        parent_item.set_selected_ring(False)
        if nodes:
            self._layers = layer + 1
            self._warning.hide()
        else:
            self._warning.setText(
                f"Thêm {len(edges)} quan hệ giữa các thực thể đã có."
                if edges
                else "Node này không còn lân cận nào chưa hiện."
            )
            self._warning.show()
        if self._selection and self._selection.kind == "node" and self._selection.node is parent:
            self._render_detail()
        self._render_status()

    def _expansion_failed(self, exc: BaseException, era: int) -> None:
        if era != self._generation:
            return
        self._expanding = ""
        self._warning.setText(str(exc) or "Không mở rộng được node này.")
        self._warning.show()
        self._render_status()

    # --- spotlight --------------------------------------------------------

    def spotlight_node(self, identifier: str) -> None:
        near = {identifier}
        lit: set[str] = set()
        for key, edge in self._edges.items():
            if edge.source == identifier or edge.target == identifier:
                near.add(edge.source)
                near.add(edge.target)
                lit.add(key)
        self._apply_spotlight(near, lit)

    def spotlight_edge(self, key: str) -> None:
        edge = self._edges.get(key)
        if edge is None:
            return
        self._apply_spotlight({edge.source, edge.target}, {key})

    def _apply_spotlight(self, near: set[str], lit: set[str]) -> None:
        for identifier, item in self._node_items.items():
            item.setOpacity(1.0 if identifier in near else FADED_OPACITY)
        for key, item in self._edge_items.items():
            item.setOpacity(1.0 if key in lit else FADED_OPACITY)
            item.set_lit(key in lit)

    def clear_spotlight(self) -> None:
        for item in self._node_items.values():
            item.setOpacity(1.0)
        for item in self._edge_items.values():
            item.setOpacity(1.0)
            item.set_lit(False)

    # --- selection and panels ---------------------------------------------

    def select_node(self, node: GraphNode) -> None:
        self._selection = _Selection(kind="node", node=node)
        self._sync_selection_rings()
        self._render_detail()

    def select_edge(self, edge: GraphEdge) -> None:
        source = self._nodes.get(edge.source)
        target = self._nodes.get(edge.target)
        if source is None or target is None:
            return
        self._selection = _Selection(kind="edge", edge=edge, ends=(source, target))
        self._sync_selection_rings()
        self._render_detail()

    def _clear_selection(self) -> None:
        self._selection = None
        self._sync_selection_rings()
        self._render_detail()

    def _sync_selection_rings(self) -> None:
        selected = (
            self._selection.node.id
            if self._selection and self._selection.kind == "node" and self._selection.node
            else ""
        )
        for identifier, item in self._node_items.items():
            item.set_selected_ring(identifier == selected)

    def _clear_layout(self, layout) -> None:
        while layout.count():
            entry = layout.takeAt(0)
            widget = entry.widget()
            if widget is not None:
                widget.deleteLater()
            elif entry.layout() is not None:
                self._clear_layout(entry.layout())

    def _render_legend(self) -> None:
        while self._legend.count() > 1:
            entry = self._legend.takeAt(0)
            widget = entry.widget()
            if widget is not None:
                widget.deleteLater()
        tally: dict[str, int] = {}
        for node in self._nodes.values():
            tally[node.type] = tally.get(node.type, 0) + 1
        self._legend_title.setVisible(bool(tally))
        for entity_type, count in sorted(tally.items(), key=lambda item: -item[1]):
            button = QPushButton(f"{entity_type}  ·  {count}")
            button.setCheckable(True)
            button.setChecked(entity_type not in self._muted)
            button.setProperty("class", "chip")
            swatch = QColor(color_of(entity_type))
            button.setIcon(_swatch_icon(swatch))
            button.clicked.connect(lambda _=False, key=entity_type: self._toggle_type(key))
            self._legend.insertWidget(self._legend.count() - 1, button)

    def _toggle_type(self, entity_type: str) -> None:
        if entity_type in self._muted:
            self._muted.discard(entity_type)
        else:
            self._muted.add(entity_type)
        for identifier, item in self._node_items.items():
            item.setVisible(self._nodes[identifier].type not in self._muted)
        self._sync_edge_visibility()
        self._render_legend()

    def _sync_edge_visibility(self) -> None:
        """Hiding a type hides the edges into it too, so nothing dangles in mid-air."""
        for key, item in self._edge_items.items():
            edge = self._edges.get(key)
            if edge is None:
                continue
            source = self._nodes.get(edge.source)
            target = self._nodes.get(edge.target)
            visible = (
                source is not None
                and target is not None
                and source.type not in self._muted
                and target.type not in self._muted
            )
            item.setVisible(visible)
            if not visible:
                item.label.setVisible(False)

    def _render_status(self) -> None:
        if not self._nodes:
            self._status.setText("")
            return
        parts = [f"{len(self._nodes)} thực thể · {len(self._edges)} quan hệ"]
        if self._layers > 1:
            parts.append(f"{self._layers} lớp")
        if self._expanding:
            parts.append("đang mở lớp mới…")
        if self._timer.isActive():
            parts.append("đang sắp xếp…")
        self._status.setText(" · ".join(parts))
        self._relayout.setVisible(self._layers > 1)
        if self._truncated:
            self._warning.setText(
                "Lớp nền đã bị cắt bớt, tăng “Số node” hoặc bấm đúp từng node để đào thêm."
            )
            self._warning.show()

    def _show_empty(self, message: str) -> None:
        self._empty.setText(message)
        self._empty.show()
        self._canvas.hide()
        self._status.setText("")
        self._warning.hide()

    def _render_detail(self) -> None:
        self._clear_layout(self._detail_layout)
        selection = self._selection
        if selection is None:
            hint = QLabel(
                "Kéo node để sắp lại chỗ. Bấm một node hoặc một đường nối để xem chi tiết, "
                "bấm đúp vào node để mở thêm một lớp lân cận quanh nó."
            )
            hint.setWordWrap(True)
            hint.setProperty("class", "muted")
            self._detail_layout.addWidget(hint)
            self._detail_layout.addStretch(1)
            return
        if selection.kind == "node" and selection.node is not None:
            self._render_node_detail(selection.node)
        elif selection.edge is not None and selection.ends is not None:
            self._render_edge_detail(selection.edge, *selection.ends)
        self._detail_layout.addStretch(1)

    def _render_node_detail(self, node: GraphNode) -> None:
        kind = QLabel(node.type)
        kind.setStyleSheet(f"color: {node.color};")
        self._detail_layout.addWidget(kind)
        title = QLabel(node.label)
        title.setWordWrap(True)
        title.setProperty("class", "subtitle")
        self._detail_layout.addWidget(title)

        meta = [f"{node.degree} quan hệ"]
        if node.layer > 0:
            meta.append(f"mở ở lớp {node.layer + 1}")
        if node.file:
            meta.append(f"nguồn: {node.file}")
        summary = QLabel(" · ".join(meta))
        summary.setWordWrap(True)
        summary.setProperty("class", "faint")
        self._detail_layout.addWidget(summary)

        if node.description:
            body = QLabel(node.description)
            body.setWordWrap(True)
            self._detail_layout.addWidget(body)

        expand = QPushButton(
            "Đang mở lớp…"
            if self._expanding == node.id
            else ("Đã mở lớp này" if node.expanded else "Mở rộng lân cận")
        )
        expand.setEnabled(not self._expanding and not node.expanded)
        expand.clicked.connect(lambda: self.expand_node(node.id))
        self._detail_layout.addWidget(expand)

        only = QPushButton("Chỉ xem lân cận")
        only.clicked.connect(lambda: self.focus_entity(node.id))
        self._detail_layout.addWidget(only)

    def _render_edge_detail(self, edge: GraphEdge, source: GraphNode, target: GraphNode) -> None:
        kind = QLabel("Quan hệ")
        kind.setProperty("class", "faint")
        self._detail_layout.addWidget(kind)
        title = QLabel(edge.label)
        title.setWordWrap(True)
        title.setProperty("class", "subtitle")
        self._detail_layout.addWidget(title)

        ends = QHBoxLayout()
        for node in (source, target):
            button = QPushButton(node.label)
            button.setToolTip(f"Chỉ xem lân cận của {node.label}")
            button.setIcon(_swatch_icon(QColor(node.color)))
            button.clicked.connect(lambda _=False, key=node.id: self.focus_entity(key))
            ends.addWidget(button)
        self._detail_layout.addLayout(ends)

        meta = []
        if edge.weight > 0:
            meta.append(f"độ mạnh {edge.weight:.1f}")
        if edge.file:
            meta.append(f"nguồn: {edge.file}")
        if meta:
            summary = QLabel(" · ".join(meta))
            summary.setWordWrap(True)
            summary.setProperty("class", "faint")
            self._detail_layout.addWidget(summary)

        if edge.description:
            body = QLabel(edge.description)
            body.setWordWrap(True)
            self._detail_layout.addWidget(body)
