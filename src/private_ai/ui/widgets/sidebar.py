"""The left rail: brand, new conversation, nav, workspaces, recents, profile.

Collapsing hides the labels rather than the rail, which is what the web app did — the
icons stay hit-targets and the user keeps their spatial memory of the list. The workspace
and conversation rows each carry an inline two-step delete, because those are the two
things people delete constantly and a dialog for each would be exhausting.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PySide6.QtCore import Qt, Signal
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
from private_ai.ui.format import elide, format_relative_time
from private_ai.ui.widgets.confirm_button import ConfirmToolButton
from private_ai.ui.widgets.profile_switcher import ProfileSwitcher

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from private_ai.ui.context import AppContext

__all__ = ["NAVIGATION", "Sidebar"]

EXPANDED_WIDTH = 284
COLLAPSED_WIDTH = 72

# The five nav destinations. Settings is last and visually separated, exactly as in the
# web app where it sat below the workspace list.
NAVIGATION: tuple[tuple[str, str, str], ...] = (
    ("chat", "Trò chuyện", "message-square-text"),
    ("workspaces", "Không gian", "layout-grid"),
    ("library", "Tài liệu", "book-open"),
    ("graph", "Tri thức", "waypoints"),
    ("settings", "Cài đặt", "settings"),
)


class _BrandMark(QWidget):
    """The three skewed bars from the CSS, drawn rather than styled."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedSize(26, 26)

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


class _WorkspaceRow(QWidget):
    chosen = Signal(str)
    deleted = Signal(str)

    def __init__(self, workspace_id: str, name: str, active: bool, parent=None) -> None:
        super().__init__(parent)
        self.workspace_id = workspace_id
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(2)

        self.button = QPushButton(f"  {elide(name, 26)}", self)
        self.button.setProperty("class", "nav-item")
        self.button.setMinimumHeight(34)
        self.button.setCheckable(True)
        self.button.setChecked(active)
        self.button.setToolTip(name)
        self.button.setIcon(
            icons.icon("folder", color=theme.token("accent") if active else None, size=15)
        )
        self.button.clicked.connect(lambda: self.chosen.emit(self.workspace_id))
        layout.addWidget(self.button, 1)

        self.delete = ConfirmToolButton(
            "trash-2",
            tooltip=f"Xóa không gian làm việc {name}",
            confirm_tooltip=f"Bấm lại để xác nhận xóa {name} và toàn bộ cuộc trò chuyện bên trong",
            parent=self,
        )
        self.delete.confirmed.connect(lambda: self.deleted.emit(self.workspace_id))
        layout.addWidget(self.delete)


class _ConversationRow(QWidget):
    chosen = Signal(str)
    deleted = Signal(str)

    def __init__(
        self,
        conversation_id: str,
        title: str,
        subtitle: str,
        active: bool,
        parent=None,
    ) -> None:
        super().__init__(parent)
        self.conversation_id = conversation_id
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(2)

        button = QPushButton(self)
        button.setProperty("class", "nav-item")
        button.setCheckable(True)
        button.setChecked(active)
        button.setMinimumHeight(44)
        button.setToolTip(title)
        button.clicked.connect(lambda: self.chosen.emit(conversation_id))
        inner = QHBoxLayout(button)
        inner.setContentsMargins(11, 4, 8, 4)
        inner.setSpacing(9)
        mark = QLabel(button)
        mark.setPixmap(icons.pixmap("message-square", 15, theme.token("faint")))
        inner.addWidget(mark)
        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(0)
        head = QLabel(elide(title, 24), button)
        head.setStyleSheet(
            f"color: {theme.token('accent-ink') if active else theme.token('text')}; "
            f"font-weight: {'700' if active else '600'};"
        )
        sub = QLabel(subtitle, button)
        sub.setProperty("class", "faint")
        copy.addWidget(head)
        copy.addWidget(sub)
        inner.addLayout(copy, 1)
        layout.addWidget(button, 1)

        # Only the open conversation offers deletion — the web app did the same, so a
        # stray click in the list cannot arm a delete on something you are not looking at.
        if active:
            remove = ConfirmToolButton(
                "x",
                tooltip="Xóa cuộc trò chuyện",
                confirm_tooltip="Bấm lại để xác nhận xóa",
                parent=self,
            )
            remove.confirmed.connect(lambda: self.deleted.emit(conversation_id))
            layout.addWidget(remove)


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
        root.setContentsMargins(16, 20, 16, 16)
        root.setSpacing(0)

        root.addLayout(self._build_header())
        root.addSpacing(18)
        root.addWidget(self._build_new_button())
        root.addSpacing(16)
        root.addLayout(self._build_nav())
        root.addSpacing(20)
        root.addWidget(self._build_lists(), 1)
        root.addSpacing(10)

        self.profiles = ProfileSwitcher(ctx, self)
        self.profiles.settingsRequested.connect(lambda: self.navigate.emit("settings"))
        root.addWidget(self.profiles)

    # ------------------------------------------------------------------ build
    def _build_header(self) -> QHBoxLayout:
        row = QHBoxLayout()
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(8)
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
        self._new.setProperty("class", "primary")
        self._new.setMinimumHeight(46)
        self._new.setIcon(icons.icon("plus", color=theme.token("on-accent"), size=18))
        self._new.setToolTip("Cuộc trò chuyện mới")
        self._new.clicked.connect(self.newConversation)
        return self._new

    def _build_nav(self) -> QVBoxLayout:
        nav = QVBoxLayout()
        nav.setContentsMargins(0, 0, 0, 0)
        nav.setSpacing(3)
        for key, label, icon_name in NAVIGATION:
            button = QPushButton(f"  {label}", self)
            button.setProperty("class", "nav-item")
            button.setCheckable(True)
            button.setIcon(icons.icon(icon_name, size=19))
            button.setToolTip(label)
            button.clicked.connect(lambda _=False, k=key: self.navigate.emit(k))
            nav.addWidget(button)
            self._nav_buttons[key] = button
        return nav

    def _build_lists(self) -> QWidget:
        holder = QWidget(self)
        box = QVBoxLayout(holder)
        box.setContentsMargins(0, 0, 0, 0)
        box.setSpacing(4)

        header = QHBoxLayout()
        header.setContentsMargins(0, 0, 0, 0)
        self._workspace_label = QLabel("KHÔNG GIAN CỦA BẠN", holder)
        self._workspace_label.setProperty("class", "section-label")
        add = QPushButton(holder)
        add.setProperty("class", "icon")
        add.setFixedSize(30, 30)
        add.setIcon(icons.icon("plus", size=16))
        add.setToolTip("Tạo không gian làm việc")
        add.clicked.connect(self.workspaceCreateRequested)
        header.addWidget(self._workspace_label, 1)
        header.addWidget(add)
        box.addLayout(header)

        self._workspace_scroll = QScrollArea(holder)
        self._workspace_scroll.setWidgetResizable(True)
        self._workspace_scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._workspace_scroll.setMaximumHeight(190)
        self._workspace_host = QWidget(self._workspace_scroll)
        self._workspace_list = QVBoxLayout(self._workspace_host)
        self._workspace_list.setContentsMargins(0, 0, 0, 0)
        self._workspace_list.setSpacing(1)
        self._workspace_list.addStretch(1)
        self._workspace_scroll.setWidget(self._workspace_host)
        box.addWidget(self._workspace_scroll)

        self._recent_label = QLabel("GẦN ĐÂY", holder)
        self._recent_label.setProperty("class", "section-label")
        box.addSpacing(8)
        box.addWidget(self._recent_label)

        self._conversation_scroll = QScrollArea(holder)
        self._conversation_scroll.setWidgetResizable(True)
        self._conversation_scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._conversation_scroll.setSizePolicy(
            QSizePolicy.Policy.Preferred, QSizePolicy.Policy.Expanding
        )
        self._conversation_host = QWidget(self._conversation_scroll)
        self._conversation_list = QVBoxLayout(self._conversation_host)
        self._conversation_list.setContentsMargins(0, 0, 0, 0)
        self._conversation_list.setSpacing(1)
        self._conversation_list.addStretch(1)
        self._conversation_scroll.setWidget(self._conversation_host)
        box.addWidget(self._conversation_scroll, 1)
        return holder

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
        self._workspace_label.setVisible(not collapsed)
        self._recent_label.setVisible(not collapsed)
        self._workspace_scroll.setVisible(not collapsed)
        self._conversation_scroll.setVisible(not collapsed)
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
        subtitle = " · ".join(part for part in (f"{count} tin nhắn", when) if part)
        row = _ConversationRow(
            conversation_id, title, subtitle, conversation_id == active_id, self._conversation_host
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
        layout.addStretch(1)
