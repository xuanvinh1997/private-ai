"""Text that nearly lines up looks worse than text that clearly does not.

Two labels forty pixels apart are two columns and the eye reads them as such. Two labels
three pixels apart are one column that something padded twice, and that is the specific
defect this guards: every view is built, every visible label is asked where its first
glyph lands, and any pair that lands *almost* on the same edge fails.

Only ``QLabel`` is measured. A checkbox's caption starts after its indicator and a
button's after its padding — those legitimately differ from a bare label beside them, so
including controls here would report nothing but false alarms.
"""

from __future__ import annotations

from collections import defaultdict

import pytest

# Under this many pixels apart, two left edges read as one column that failed rather than
# as two columns. Zero apart is correct; ten or more is a deliberate indent.
NEAR_MISS_PX = 8

# Classes whose padding is part of the shape rather than the text column: a pill's inset
# belongs to the pill, and an avatar is a tile with a letter centred in it.
SHAPED = frozenset(
    {
        "pill",
        "chip",
        "chip-active",
        "badge-success",
        "badge-warn",
        "badge-danger",
        "avatar",
        "avatar-lg",
        "code",
    }
)

VIEWS = (
    "library_view.LibraryView",
    "workspaces_view.WorkspacesView",
    "settings_view.SettingsView",
    "models_view.ModelsView",
    "mcp_view.McpView",
    "memory_view.MemoryView",
    "providers_view.ProvidersView",
    "skills_view.SkillsView",
)


def _load(path: str):
    import importlib

    module_name, class_name = path.split(".")
    module = importlib.import_module(f"private_ai.ui.views.{module_name}")
    return getattr(module, class_name)


def _text_edges(view) -> dict[int, list[str]]:
    from PySide6.QtWidgets import QLabel

    edges: dict[int, list[str]] = defaultdict(list)
    for label in view.findChildren(QLabel):
        if not label.isVisible():
            continue
        text = (label.text() or "").strip()
        # Rich text carries its own indents; an empty label has no edge to speak of.
        if not text or text.startswith("<"):
            continue
        css = str(label.property("class") or "")
        if css in SHAPED:
            continue
        corner = label.mapTo(view, label.rect().topLeft())
        if corner.x() < 0 or corner.y() < 0:
            continue
        edges[corner.x()].append(f"{css or 'unclassed'}:{text[:24]}")
    return edges


def _near_misses(edges: dict[int, list[str]]) -> list[list[int]]:
    clusters: list[list[int]] = []
    for key in sorted(edges):
        if clusters and key - clusters[-1][-1] < NEAR_MISS_PX:
            clusters[-1].append(key)
        else:
            clusters.append([key])
    return [cluster for cluster in clusters if len(cluster) > 1]


@pytest.fixture
def built(services, workspace_id, qapp):
    from private_ai.ui import theme
    from private_ai.ui.context import AppContext

    theme.apply_theme(qapp, "light", "normal")
    context = AppContext(services=services)
    context.workspace_id = workspace_id
    made = []

    def build(path: str):
        view = _load(path)(context)
        view.resize(980, 760)
        view.show()
        qapp.processEvents()
        qapp.processEvents()
        made.append(view)
        return view

    yield build
    for view in made:
        view.close()


@pytest.mark.parametrize("path", VIEWS)
async def test_labels_share_their_columns(built, path: str) -> None:
    # Async because the views schedule their first load on construction, and that needs a
    # running loop the way the real application has one.
    view = built(path)
    misses = _near_misses(_text_edges(view))
    if misses:
        edges = _text_edges(view)
        detail = "; ".join(
            f"x={cluster} -> " + " | ".join(edges[key][0] for key in cluster) for cluster in misses
        )
        pytest.fail(f"{path}: text edges that almost agree: {detail}")


def test_the_header_stack_is_one_column(qapp) -> None:
    """The eyebrow, the title and the blurb are one column, on every page."""
    from PySide6.QtWidgets import QLabel, QVBoxLayout, QWidget

    from private_ai.ui import theme

    theme.apply_theme(qapp, "light", "normal")
    host = QWidget()
    column = QVBoxLayout(host)
    column.setContentsMargins(0, 0, 0, 0)
    column.setSpacing(theme.SPACE["3xs"])
    labels = []
    for css in ("section-label", "title", "muted", "heading", "card-title", "body", "faint"):
        label = QLabel("Tai lieu")
        label.setProperty("class", css)
        column.addWidget(label)
        labels.append((css, label))
    host.resize(420, 320)
    host.show()
    qapp.processEvents()

    offsets = {css: label.contentsMargins().left() for css, label in labels}
    host.close()
    stray = {css: pad for css, pad in offsets.items() if pad != 0}
    assert not stray, f"these classes indent their own text out of the column: {stray}"


# --- the places the view sweep above cannot reach -------------------------
#
# It walks the eight top-level views. The context rail is built inside ChatView, and the
# profile popup is a free-floating QFrame owned by no view at all, so both went unchecked
# until a screenshot showed them broken.


async def test_the_rail_document_row_is_one_line(services, workspace_id, qapp) -> None:
    """A rail row is one line, and a long filename is elided rather than wrapped.

    The state used to be a second line under the name, so three ready documents wrote
    "Sẵn sàng" three times down a 296px column; it is a pip and a tooltip now. What has to
    hold is that no filename can win itself a second line and push every row below it out
    of rhythm — the rail is a fixed column of single-line rows.
    """
    from PySide6.QtGui import QFontMetrics

    from private_ai.ui import theme
    from private_ai.ui.views.chat_view import _RailDocument

    theme.apply_theme(qapp, "light", "normal")
    long_name = "bao-cao-tong-ket-quy-bon-nam-hai-nghin-khong-tram-hai-muoi-lam-ban-cuoi.pdf"
    rows = []
    for filename in ("2201.11903.pdf", long_name):
        row = _RailDocument()
        row.set_document("d1", filename, "Sẵn sàng", busy=False, pip_state="ready")
        row.resize(264, row.sizeHint().height())
        row.show()
        qapp.processEvents()
        rows.append(row)

    short, long_row = rows
    metrics = QFontMetrics(long_row._name.font())
    painted = metrics.horizontalAdvance(long_row._name.text())
    heights = [row.height() for row in rows]
    name_x = [row._name.mapTo(row, row._name.rect().topLeft()).x() for row in rows]
    tooltip = short.toolTip()
    for row in rows:
        row.close()

    assert heights[0] == heights[1], (
        f"a long filename grew its row: {heights[1]}px against {heights[0]}px"
    )
    assert painted <= long_row._name.width() + 1, (
        f"filename paints {painted}px into a {long_row._name.width()}px column"
    )
    assert name_x[0] == name_x[1], f"names start at {name_x[0]} and {name_x[1]}"
    assert "Sẵn sàng" in tooltip, "the state left the row without reaching the tooltip"


async def test_popup_rows_share_a_caption_column(services, workspace_id, qapp) -> None:
    """A QPushButton centres its icon-and-text group unless told otherwise.

    Left un-told, a column of them fans out around the centre line and every caption
    starts somewhere different — which is the opposite of what a menu is.
    """
    from PySide6.QtWidgets import QVBoxLayout, QWidget

    from private_ai.ui import theme
    from private_ai.ui.widgets.profile_switcher import _menu_row

    theme.apply_theme(qapp, "light", "normal")
    host = QWidget()
    box = QVBoxLayout(host)
    box.setContentsMargins(8, 8, 8, 8)
    box.setSpacing(2)
    rows = []
    # The first has no icon of its own: it must still reserve the slot.
    for text, icon_name in (("Bạn", ""), ("Thêm hồ sơ", "user-plus"), ("Cài đặt", "settings")):
        button = _menu_row(text, icon_name, host)
        box.addWidget(button)
        rows.append(button)
    host.resize(250, 160)
    host.show()
    qapp.processEvents()

    lefts = {button.mapTo(host, button.rect().topLeft()).x() for button in rows}
    sizes = {button.iconSize().width() for button in rows}
    heights = {button.height() for button in rows}
    host.close()

    assert len(lefts) == 1, f"popup rows start at different x: {lefts}"
    assert len(sizes) == 1, f"icon slots differ, so captions cannot align: {sizes}"
    assert len(heights) == 1, f"popup rows disagree on height: {heights}"


def test_no_control_rule_caps_a_two_line_tool_button(qapp) -> None:
    """The 32px cap is right for a toolbar and wrong for anything with two lines in it.

    Rather than loosen the cap and let single-line tool buttons drift off the baseline,
    multi-line content belongs in a composite row — so this asserts the cap is still
    there, and the rail test above asserts nothing multi-line is subject to it.
    """
    from PySide6.QtWidgets import QToolButton

    from private_ai.ui import theme

    theme.apply_theme(qapp, "light", "normal")
    button = QToolButton()
    button.setText("Một dòng")
    button.ensurePolished()
    assert button.maximumHeight() <= theme.CONTROL_HEIGHT


@pytest.mark.parametrize("path", VIEWS)
async def test_the_empty_state_does_not_stretch_the_header(built, path: str) -> None:
    """An empty list must not inflate the title.

    Every list page hides its scroll area when it has nothing to show, and the scroll area
    is the only child holding the column's vertical stretch. Without a second home for that
    stretch Qt shares the surplus among the remaining widgets, and a 26px title comes back
    132px tall with the eyebrow floating a hundred pixels above the blurb.
    """
    from PySide6.QtWidgets import QLabel

    view = built(path)
    empty = getattr(view, "_empty", None)
    scroll = getattr(view, "_scroll", None)
    if empty is None or scroll is None:
        pytest.skip(f"{path} has no empty state to force")
    empty.setText("Không có gì ở đây.\nThử lại sau.")
    empty.show()
    scroll.hide()
    view.layout().activate()

    overrun = []
    for label in view.findChildren(QLabel):
        if str(label.property("class") or "") not in {"section-label", "title"}:
            continue
        if not label.isVisible() or not (label.text() or "").strip():
            continue
        hint = label.sizeHint().height()
        if hint and label.height() > hint + NEAR_MISS_PX:
            overrun.append(f"{label.text()[:24]!r} {label.height()}px for a {hint}px line")
    assert not overrun, f"{path}: the empty state stretched the header: " + "; ".join(overrun)


async def test_sidebar_rows_have_a_height(services, workspace_id, qapp) -> None:
    """Every list row in the rail must be as tall as the stylesheet says it is.

    A row builds its button, parents it, fills it — and then has to put it in the row's own
    layout. Forgetting that last step leaves the row with an empty layout, a
    ``minimumSizeHint`` of zero and a height of zero: the widgets all exist, ``isVisible``
    is true, and the list paints as an empty gap. Nothing else in the suite notices.
    """
    from private_ai.core import repositories
    from private_ai.ui import theme
    from private_ai.ui.context import AppContext
    from private_ai.ui.widgets.sidebar import Sidebar, _ConversationRow, _WorkspaceRow

    theme.apply_theme(qapp, "light", "normal")
    context = AppContext(services=services)
    context.workspace_id = workspace_id
    rail = Sidebar(context)
    rail.resize(252, 860)
    rail.show()
    rail.set_workspaces(await repositories.list_workspaces(services.database), workspace_id)
    rail.set_conversations(
        await repositories.list_conversations(services.database, workspace_id), ""
    )
    qapp.processEvents()
    qapp.processEvents()

    flat = []
    for kind in (_WorkspaceRow, _ConversationRow):
        for row in rail.findChildren(kind):
            if row.minimumSizeHint().height() <= 0:
                flat.append(f"{kind.__name__} minimumSizeHint={row.minimumSizeHint().height()}")
    rail.close()
    assert not flat, "rail rows with no height of their own: " + "; ".join(flat)
