"""The left rail: brand, new conversation, nav, workspaces, recents, profile.

Collapsing hides the labels rather than the rail, which is what the web app did — the
icons stay hit-targets and the user keeps their spatial memory of the list. The workspace
and conversation rows each carry an inline two-step delete, because those are the two
things people delete constantly and a dialog for each would be exhausting.

Everything below the nav lives in one scroll region. Two stacked scrollers in a 250px rail
meant a short workspace list still reserved its cap in pixels, and a second scrollbar
appeared a centimetre from the first; one region lets the recents have whatever the
workspaces do not use.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QFontMetrics
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import icons, theme
from private_ai.ui.format import format_relative_time
from private_ai.ui.widgets.confirm_button import ConfirmToolButton
from private_ai.ui.widgets.profile_switcher import ProfileSwitcher

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.ui.context import AppContext

__all__ = ["NAVIGATION", "Sidebar"]

EXPANDED_WIDTH = 252
COLLAPSED_WIDTH = 64

# The stylesheet indents a rail row's own contents; the section labels above the lists have
# to start on that same edge or the rail reads as two misaligned columns.
LABEL_INDENT = theme.SPACE["sm"] + theme.SPACE["3xs"]

# Every row reserves this much for its delete affordance whether or not the button is
# showing, so revealing one on hover never reflows the title next to it.
# Matches the stylesheet's icon-button footprint, so the berth neither clips the
# button nor leaves a gap when it is hidden.
ACTION_SLOT = 30

# The berth at the end of a conversation row, wide enough for the longest relative time
# ``format_relative_time`` produces ("Bây giờ", "59 phút") as well as the delete button.
META_SLOT = theme.SPACE["4xl"] + theme.SPACE["sm"]

# The stylesheet's icon-button size, needed here to centre the lone toggle in a collapsed
# rail rather than let it hang off the right edge.
ICON_BUTTON = 30

# The five nav destinations. Settings is last and visually separated, exactly as in the
# web app where it sat below the workspace list.
NAVIGATION: tuple[tuple[str, str, str], ...] = (
    ("chat", "Trò chuyện", "message-square-text"),
    ("workspaces", "Không gian", "layout-grid"),
    ("library", "Tài liệu", "book-open"),
    ("graph", "Tri thức", "waypoints"),
    ("settings", "Cài đặt", "settings"),
)


# The mark is a logo at its own size, which is why it is the one dimension here that does
# not land on the 4px rhythm: it matches the badge height so the wordmark beside it sits
# on the same line as every other 26px shape in the shell.
_MARK_SIZE = theme.BADGE_HEIGHT


class _BrandMark(QWidget):
    """The three skewed bars from the CSS, drawn rather than styled."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedSize(_MARK_SIZE, _MARK_SIZE)

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        from PySide6.QtGui import QColor, QPainter

        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        base = QColor(theme.token("accent"))
        for index, (height, alpha) in enumerate(((10, 140), (18, 199), (25, 255))):
            color = QColor(base)
            color.setAlpha(alpha)
            painter.setBrush(color)
            painter.drawRoundedRect(index * 8, 26 - height, 5, height, 2, 2)
        painter.end()


class _ElidedLabel(QLabel):
    """Trims to the width the rail actually gives it, rather than a character guess.

    The old rows cut every title at a fixed count, which threw away real estate at 252px
    and would have overflowed at any wider setting. Eliding at paint time means one row
    definition works at every rail width and every root font size.
    """

    def __init__(self, text: str = "", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._full = text
        self.setMinimumWidth(0)
        self.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self._apply()

    def set_full_text(self, text: str) -> None:
        self._full = text
        self.setToolTip(text)
        self._apply()

    def resizeEvent(self, event) -> None:  # noqa: N802 - Qt override
        super().resizeEvent(event)
        self._apply()

    def _apply(self) -> None:
        width = max(self.width(), 0)
        metrics = QFontMetrics(self.font())
        super().setText(metrics.elidedText(self._full, Qt.TextElideMode.ElideRight, width))


class _ActionSlot(QWidget):
    """A fixed-width berth at the end of a row, holding one thing at a time.

    The delete button hides when the row is neither hovered nor current, but the berth
    stays, so the title beside it keeps one width for the life of the row. A row may also
    hand over something to show while at rest — a conversation shows its age there — and
    the two swap in place rather than each claiming width of their own.
    """

    def __init__(
        self,
        button: QPushButton,
        *,
        resting: QWidget | None = None,
        width: int = ACTION_SLOT,
        parent: QWidget | None = None,
    ) -> None:
        # Keyword-only past the button: this used to be ``(button, parent)``, and growing it
        # silently turned an existing call's parent into the resting widget — which reparented
        # a row's own button inside itself and hung the layout instead of raising.
        super().__init__(parent)
        self.setFixedWidth(width)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        # Both stretch: a hidden widget takes no space, so whichever is showing fills the
        # berth and the row's right edge never moves.
        if resting is not None:
            layout.addWidget(resting, 1)
        layout.addWidget(button, 1)
        self.button = button
        self.resting = resting


class _RailRow(QWidget):
    """Shared hover bookkeeping for the two list row types."""

    def __init__(self, pinned: bool, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._pinned = pinned
        self._action: ConfirmToolButton | None = None
        self._resting: QWidget | None = None

    def _watch(self, action: ConfirmToolButton, resting: QWidget | None = None) -> None:
        self._action = action
        self._resting = resting
        self._show_action(self._pinned)

    def _show_action(self, showing: bool) -> None:
        if self._action is not None:
            self._action.setVisible(showing)
        if self._resting is not None:
            self._resting.setVisible(not showing)

    def enterEvent(self, event) -> None:  # noqa: N802 - Qt override
        self._show_action(True)
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:  # noqa: N802 - Qt override
        # An armed button must survive the pointer leaving, or the second click of the
        # two-step confirm has nothing to land on.
        if self._action is not None and not self._action.is_armed():
            self._show_action(self._pinned)
        super().leaveEvent(event)


class _WorkspaceRow(_RailRow):
    chosen = Signal(str)
    deleted = Signal(str)

    def __init__(self, workspace_id: str, name: str, active: bool, parent=None) -> None:
        super().__init__(active, parent)
        self.workspace_id = workspace_id
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        self.button = QPushButton(self)
        self.button.setProperty("class", "rail-item")
        self.button.setCheckable(True)
        self.button.setChecked(active)
        self.button.setToolTip(name)
        self.button.clicked.connect(lambda: self.chosen.emit(self.workspace_id))
        layout.addWidget(self.button, 1)

        inner = QHBoxLayout(self.button)
        inner.setContentsMargins(theme.SPACE["sm"], 0, theme.SPACE["3xs"], 0)
        inner.setSpacing(theme.SPACE["sm"])
        mark = QLabel(self.button)
        mark.setPixmap(icons.pixmap("folder", 15, theme.token("accent" if active else "muted")))
        inner.addWidget(mark)
        label = _ElidedLabel(name, self.button)
        label.setProperty("class", "rail-active" if active else "body")
        inner.addWidget(label, 1)

        self.delete = ConfirmToolButton(
            "trash-2",
            tooltip=f"Xóa không gian làm việc {name}",
            confirm_tooltip=f"Bấm lại để xác nhận xóa {name} và toàn bộ cuộc trò chuyện bên trong",
            parent=self.button,
        )
        self.delete.confirmed.connect(lambda: self.deleted.emit(self.workspace_id))
        inner.addWidget(_ActionSlot(self.delete, parent=self.button))
        self._watch(self.delete)


class _ConversationRow(_RailRow):
    chosen = Signal(str)
    deleted = Signal(str)

    def __init__(
        self,
        conversation_id: str,
        title: str,
        when: str,
        detail: str,
        active: bool,
        parent=None,
    ) -> None:
        super().__init__(active, parent)
        self.conversation_id = conversation_id
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        button = QPushButton(self)
        # The same row shape as a workspace: the recents used to be 46px and two lines
        # deep, the second line reading "N tin nhắn · X giờ" down every row — a column of
        # the same three words that told the eye nothing about any one conversation.
        button.setProperty("class", "rail-item")
        button.setCheckable(True)
        button.setChecked(active)
        button.setToolTip(detail)
        button.clicked.connect(lambda: self.chosen.emit(conversation_id))
        inner = QHBoxLayout(button)
        inner.setContentsMargins(theme.SPACE["sm"], 0, theme.SPACE["3xs"], 0)
        inner.setSpacing(theme.SPACE["sm"])
        mark = QLabel(button)
        mark.setPixmap(
            icons.pixmap("message-square", 15, theme.token("accent" if active else "muted"))
        )
        inner.addWidget(mark)
        head = _ElidedLabel(title, button)
        # The row is rebuilt on every selection change, so the emphasis can be a class
        # swap rather than an inline weight — which keeps the palette with the stylesheet.
        head.setProperty("class", "rail-active" if active else "body")
        inner.addWidget(head, 1)

        # Age at rest, delete on hover, both in the one berth: showing the age in a column
        # of its own would have cost the title a third of its width.
        age = QLabel(when, button)
        age.setProperty("class", "faint")
        age.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        age.setToolTip(detail)
        remove = ConfirmToolButton(
            "x",
            tooltip="Xóa cuộc trò chuyện",
            confirm_tooltip="Bấm lại để xác nhận xóa",
            parent=button,
        )
        remove.confirmed.connect(lambda: self.deleted.emit(conversation_id))
        inner.addWidget(_ActionSlot(remove, resting=age, width=META_SLOT, parent=button))
        layout.addWidget(button, 1)
        # The open conversation keeps its delete on screen — it is the one row a keyboard
        # user can tab into without a pointer to reveal anything.
        self._watch(remove, age)


class Sidebar(QWidget):
    navigate = Signal(str)
    newConversation = Signal()
    workspaceChosen = Signal(str)
    workspaceDeleted = Signal(str)
    workspaceCreateRequested = Signal()
    conversationChosen = Signal(str)
    conversationDeleted = Signal(str)
    collapsedChanged = Signal(bool)

    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.ctx = ctx
        self.setObjectName("Sidebar")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.setFixedWidth(EXPANDED_WIDTH)
        self._collapsed = False
        self._nav_buttons: dict[str, QPushButton] = {}

        root = QVBoxLayout(self)
        root.setContentsMargins(
            theme.SPACE["md"], theme.SPACE["md"], theme.SPACE["md"], theme.SPACE["md"]
        )
        root.setSpacing(0)

        root.addLayout(self._build_header())
        root.addSpacing(theme.SPACE["md"])
        root.addWidget(self._build_new_button())
        root.addSpacing(theme.SPACE["md"])
        root.addLayout(self._build_nav())
        root.addSpacing(theme.SPACE["md"])
        root.addWidget(self._build_lists(), 1)
        root.addSpacing(theme.SPACE["sm"])

        self.profiles = ProfileSwitcher(ctx, self)
        self.profiles.settingsRequested.connect(lambda: self.navigate.emit("settings"))
        root.addWidget(self.profiles)

    # ------------------------------------------------------------------ build
    def _build_header(self) -> QHBoxLayout:
        row = self._header = QHBoxLayout()
        # The 4px band lifts the row to 38px, which puts the toggle on the same baseline as
        # the topbar's controls — the two meet at the seam between rail and stage.
        row.setContentsMargins(0, theme.SPACE["2xs"], 0, theme.SPACE["2xs"])
        row.setSpacing(theme.SPACE["sm"])
        self._mark = _BrandMark(self)
        row.addWidget(self._mark)
        self._wordmark = QLabel(self)
        self._wordmark.setTextFormat(Qt.TextFormat.RichText)
        self._wordmark.setText(
            f'<span style="color:{theme.token("ink")};font-weight:760;letter-spacing:1px;">'
            f'PRIVATE</span><span style="color:{theme.token("accent")};font-weight:760;">AI</span>'
        )
        row.addWidget(self._wordmark, 1)

        self._toggle = QPushButton(self)
        self._toggle.setProperty("class", "icon")
        self._toggle.setIcon(icons.icon("panel-left-close", size=18))
        self._toggle.setToolTip("Thu gọn thanh bên")
        self._toggle.clicked.connect(lambda: self.set_collapsed(not self._collapsed))
        row.addWidget(self._toggle)
        return row

    def _build_new_button(self) -> QPushButton:
        self._new = QPushButton("  Cuộc trò chuyện mới", self)
        self._new.setProperty("class", "cta")
        self._new.setIcon(icons.icon("plus", color=theme.token("on-accent"), size=18))
        self._new.setToolTip("Cuộc trò chuyện mới")
        self._new.clicked.connect(self.newConversation)
        return self._new

    def _build_nav(self) -> QVBoxLayout:
        nav = QVBoxLayout()
        nav.setContentsMargins(0, 0, 0, 0)
        # Destinations are one group; a gap wide enough to read as separation between them
        # only pushes the lists down. Adjacency is what says "these five belong together".
        nav.setSpacing(theme.SPACE["3xs"])
        for key, label, icon_name in NAVIGATION:
            button = QPushButton(f"  {label}", self)
            button.setProperty("class", "rail-item")
            button.setCheckable(True)
            button.setIcon(icons.icon(icon_name, size=18))
            button.setToolTip(label)
            button.clicked.connect(lambda _=False, k=key: self.navigate.emit(k))
            nav.addWidget(button)
            self._nav_buttons[key] = button
        return nav

    @staticmethod
    def _section_label(text: str, parent: QWidget) -> QLabel:
        label = QLabel(text, parent)
        label.setProperty("class", "section-label")
        return label

    def _build_lists(self) -> QWidget:
        self._lists_scroll = QScrollArea(self)
        self._lists_scroll.setWidgetResizable(True)
        self._lists_scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._lists_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self._lists_scroll.setSizePolicy(QSizePolicy.Policy.Preferred, QSizePolicy.Policy.Expanding)

        holder = QWidget(self._lists_scroll)
        self._lists_holder = holder
        box = QVBoxLayout(holder)
        box.setContentsMargins(0, 0, 0, 0)
        box.setSpacing(0)

        # The stylesheet pads .section-label to zero, so the indent that lines an eyebrow
        # up with the row titles below it has to come from the layout.
        header = QHBoxLayout()
        header.setContentsMargins(LABEL_INDENT, 0, 0, theme.SPACE["2xs"])
        header.setSpacing(theme.SPACE["xs"])
        self._workspace_label = self._section_label("KHÔNG GIAN CỦA BẠN", holder)
        add = QPushButton(holder)
        add.setProperty("class", "icon")
        add.setIcon(icons.icon("plus", size=15))
        add.setToolTip("Tạo không gian làm việc")
        add.clicked.connect(self.workspaceCreateRequested)
        header.addWidget(self._workspace_label, 1)
        header.addWidget(add)
        box.addLayout(header)

        self._workspace_host = QWidget(holder)
        self._workspace_list = QVBoxLayout(self._workspace_host)
        self._workspace_list.setContentsMargins(0, 0, 0, 0)
        self._workspace_list.setSpacing(theme.SPACE["3xs"])
        box.addWidget(self._workspace_host)

        self._recent_label = self._section_label("GẦN ĐÂY", holder)
        recent = QHBoxLayout()
        recent.setContentsMargins(LABEL_INDENT, theme.SPACE["md"], 0, theme.SPACE["2xs"])
        recent.addWidget(self._recent_label, 1)
        box.addLayout(recent)

        self._conversation_host = QWidget(holder)
        self._conversation_list = QVBoxLayout(self._conversation_host)
        self._conversation_list.setContentsMargins(0, 0, 0, 0)
        self._conversation_list.setSpacing(theme.SPACE["3xs"])
        box.addWidget(self._conversation_host)
        box.addStretch(1)

        self._lists_scroll.setWidget(holder)
        return self._lists_scroll

    # -------------------------------------------------------------------- API
    def set_active_view(self, key: str) -> None:
        for name, button in self._nav_buttons.items():
            button.setChecked(name == key)

    def set_collapsed(self, collapsed: bool) -> None:
        if collapsed == self._collapsed:
            return
        self._collapsed = collapsed
        self.setFixedWidth(COLLAPSED_WIDTH if collapsed else EXPANDED_WIDTH)
        self._toggle.setIcon(
            icons.icon("panel-left-open" if collapsed else "panel-left-close", size=18)
        )
        self._toggle.setToolTip("Mở rộng thanh bên" if collapsed else "Thu gọn thanh bên")
        self._wordmark.setVisible(not collapsed)
        # Mark plus toggle need 64px of row; a collapsed rail has 40. The toggle is the only
        # way back out, so it is the one that stays, centred by the row's right inset.
        self._mark.setVisible(not collapsed)
        inset = (COLLAPSED_WIDTH - 2 * theme.SPACE["md"] - ICON_BUTTON) // 2
        self._header.setContentsMargins(
            0, theme.SPACE["2xs"], inset if collapsed else 0, theme.SPACE["2xs"]
        )
        # The scroller itself stays: it is the only stretching item in the column, and a
        # hidden one leaves the layout with nothing to give the slack to, which spreads the
        # nav icons down the whole rail. Emptying it is enough.
        self._lists_holder.setVisible(not collapsed)
        self.profiles.setVisible(not collapsed)
        self._new.setText("" if collapsed else "  Cuộc trò chuyện mới")
        for key, label, _ in NAVIGATION:
            self._nav_buttons[key].setText("" if collapsed else f"  {label}")
        self.collapsedChanged.emit(collapsed)

    def is_collapsed(self) -> bool:
        return self._collapsed

    def set_workspaces(self, workspaces: Sequence, active_id: str = "") -> None:
        self._fill(
            self._workspace_list,
            workspaces,
            "Chưa có không gian làm việc",
            lambda item: self._workspace_row(item, active_id),
        )

    def set_conversations(self, conversations: Sequence, active_id: str = "") -> None:
        self._fill(
            self._conversation_list,
            conversations,
            "Chưa có cuộc trò chuyện",
            lambda item: self._conversation_row(item, active_id),
        )

    def set_online(self, online: bool) -> None:
        self.profiles.set_online(online)

    def refresh_profiles(self) -> None:
        self.profiles.refresh()

    # -------------------------------------------------------------- internals
    def _workspace_row(self, item, active_id: str) -> QWidget:
        workspace_id = str(getattr(item, "id", "") or "")
        name = str(getattr(item, "name", "") or "Không gian")
        row = _WorkspaceRow(workspace_id, name, workspace_id == active_id, self._workspace_host)
        row.chosen.connect(self.workspaceChosen)
        row.deleted.connect(self.workspaceDeleted)
        return row

    def _conversation_row(self, item, active_id: str) -> QWidget:
        conversation_id = str(getattr(item, "id", "") or "")
        title = str(getattr(item, "title", "") or "Cuộc trò chuyện")
        count = int(getattr(item, "message_count", 0) or 0)
        when = format_relative_time(str(getattr(item, "updated_at", "") or ""))
        # The count is third-rank: it never distinguishes two rows in a list where every
        # entry says "2 tin nhắn". The tooltip keeps it for whoever wants it.
        detail = "\n".join(
            part
            for part in (
                title,
                " · ".join(
                    piece for piece in (f"{count} tin nhắn" if count else "", when) if piece
                ),
            )
            if part
        )
        row = _ConversationRow(
            conversation_id,
            title,
            when,
            detail,
            conversation_id == active_id,
            self._conversation_host,
        )
        row.chosen.connect(self.conversationChosen)
        row.deleted.connect(self.conversationDeleted)
        return row

    @staticmethod
    def _fill(layout: QVBoxLayout, items: Sequence, empty_text: str, make) -> None:
        while layout.count():
            entry = layout.takeAt(0)
            widget = entry.widget()
            if widget is not None:
                widget.deleteLater()
        if not items:
            empty = QLabel(empty_text)
            empty.setProperty("class", "empty")
            empty.setAlignment(Qt.AlignmentFlag.AlignLeft)
            layout.addWidget(empty)
        else:
            for item in items:
                layout.addWidget(make(item))
