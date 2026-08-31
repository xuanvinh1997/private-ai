"""The chat screen — message list, composer, and the context rail beside it.

Three behaviours here are not cosmetic and are the reason this file is not shorter.

*Token coalescing.* ``AgentRunner.stream`` can yield a token every few milliseconds. One
repaint per token means one full re-layout of a growing rich-text document per token, and
the view stops responding to the Stop button. Tokens are accumulated in a plain string and
painted on a 16 ms timer — the same trick the web app used ``requestAnimationFrame`` for.

*Conditional autoscroll.* Following the bottom is right only while the reader is already
at the bottom. If they have scrolled up to re-read something, the answer must not yank
them back; a button offers the trip instead.

*Partial answers survive Stop.* Cancelling the task throws ``CancelledError`` into the
runner, which writes down whatever the model had already said. So the view keeps its
partial bubble rather than clearing it — the two halves have to agree or the text appears
to vanish and then reappear on the next load.
"""

from __future__ import annotations

import asyncio
import contextlib
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QGuiApplication, QTextCursor
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPlainTextEdit,
    QProgressBar,
    QScrollArea,
    QSizePolicy,
    QTextBrowser,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.config import is_unified_memory
from private_ai.core import repositories
from private_ai.core.preferences import (
    RETRIEVAL_STRATEGY_KEY,
    WEB_SEARCH_ENABLED_KEY,
    write_app_preference_async,
)
from private_ai.core.schemas import ConversationCreate, RetrievalStrategyName
from private_ai.ui import markdown as md
from private_ai.ui import theme
from private_ai.ui.a11y import describe
from private_ai.ui.audio.capture import STATE_RECORDING, STATE_TRANSCRIBING, MicrophoneCapture
from private_ai.ui.dialogs.document_viewer import DocumentViewer
from private_ai.ui.dialogs.upload_dialog import UploadDialog, accepted_paths
from private_ai.ui.format import format_bytes, format_file_size
from private_ai.ui.icons import icon
from private_ai.ui.models.documents_model import (
    document_progress,
    document_status_text,
    is_document_busy,
)
from private_ai.ui.widgets.model_picker import ModelEntry, ModelPicker
from private_ai.ui.widgets.reasoning_trail import ReasoningTrail
from private_ai.ui.widgets.status_pip import StatusPipLabel
from private_ai.ui.widgets.strategy_picker import StrategyPicker, strategy_label

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from PySide6.QtGui import QDragEnterEvent, QDropEvent, QResizeEvent

    from private_ai.ui.context import AppContext

__all__ = ["ChatView"]

REPAINT_INTERVAL_MS = 16  # one frame; see the module docstring
NEAR_BOTTOM_PX = 80
CONTEXT_POLL_MS = 1_200
RAIL_WIDTH = 296
# A status value sits under its caption, on the column the caption's own text starts on:
# the pip's width plus the gap ``StatusPipLabel`` puts after it.
STATUS_TEXT_INSET = theme.SPACE["sm"] + theme.SPACE["xs"]
RAIL_DOCUMENTS = 3
# The citation mark is a glyph in a fixed gutter, so every filename beside it starts on
# one edge. The composer is three lines of body text before it starts scrolling.
_MARK_GLYPH = theme.SPACE["lg"]
_COMPOSER_HEIGHT = theme.SPACE["4xl"] + theme.SPACE["3xl"]

STARTER_PROMPTS = (
    "Tóm tắt các tài liệu mới trong thư viện",
    "Giúp tôi lên kế hoạch công việc hôm nay",
    "Tìm lại thông tin tôi đã lưu về dự án",
)

NEED_MODEL = "Hãy cài hoặc chọn một mô hình trước khi gửi tin nhắn."
NEED_WORKSPACE = "Hãy tạo một không gian làm việc trước."
THINKING = "Đang suy nghĩ"
SCROLL_LATEST = "Cuộn tới trả lời mới nhất"
ON_DEVICE_TITLE = "Đang chạy trên thiết bị"
ON_DEVICE_BODY = "Nội dung được gửi tới runtime cục bộ."
REMOTE_TITLE = "Đang dùng máy chủ đã chọn"
REMOTE_BODY = "Nội dung trò chuyện và tài liệu liên quan có thể rời khỏi máy này."


def _initials(name: str) -> str:
    parts = [part for part in name.split() if part]
    if not parts:
        return "B"
    if len(parts) == 1:
        return parts[0][:2].upper()
    return (parts[0][0] + parts[-1][0]).upper()


def _citation_fields(item: Any) -> dict[str, Any]:
    """Citations arrive as dicts or as ``Citation`` models depending on the graph node."""
    if isinstance(item, dict):
        source = item
    elif hasattr(item, "model_dump"):
        source = item.model_dump()
    else:
        return {}
    page = source.get("page")
    return {
        "document_id": str(source.get("document_id") or ""),
        "filename": str(source.get("filename") or ""),
        "page": int(page) if isinstance(page, int | float) else None,
        "snippet": str(source.get("snippet") or ""),
    }


class _FittedBrowser(QTextBrowser):
    """A read-only rich-text block that is exactly as tall as its content.

    QTextBrowser is a scroll area by default; inside a message list it has to behave like
    a label instead, or every bubble gets its own scrollbar.
    """

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setOpenExternalLinks(True)
        self.setFrameShape(QFrame.Shape.NoFrame)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        # Qt's rich-text CSS has no max-width, so the reading measure is capped here.
        self.setMaximumWidth(md.READING_MEASURE_PX)
        self.document().documentLayout().documentSizeChanged.connect(self._fit)

    def _fit(self, size: Any) -> None:
        height = int(size.height()) + 2 * self.frameWidth() + 4
        if height != self.height():
            self.setFixedHeight(max(height, 24))


class _RailDocument(QFrame):
    """One document in the context rail: a mark, a filename and its state beneath it.

    This was a ``QToolButton`` carrying ``"name\nstatus"`` with the icon set beside the
    text. Qt centres that icon against the whole two-line block, so it landed between the
    lines, and the control's height cap left the two lines overlapping it. A row with a
    name and a state under it is a row, not a button with a newline in it.
    """

    activated = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._document_id = ""
        self._busy = False
        self.setCursor(Qt.CursorShape.PointingHandCursor)

        row = QHBoxLayout(self)
        row.setContentsMargins(0, theme.SPACE["3xs"], 0, theme.SPACE["3xs"])
        row.setSpacing(theme.SPACE["sm"])

        self._mark = QLabel(self)
        self._mark.setPixmap(icon("book-open", size=_MARK_GLYPH).pixmap(_MARK_GLYPH, _MARK_GLYPH))
        self._mark.setFixedWidth(_MARK_GLYPH)
        # The mark belongs to the filename, so it hangs from the first line rather than
        # floating to the middle of the two.
        row.addWidget(self._mark, 0, Qt.AlignmentFlag.AlignTop)

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        self._name = QLabel(self)
        self._name.setProperty("class", "body")
        self._name.setWordWrap(True)
        self._state = QLabel(self)
        self._state.setProperty("class", "muted")
        copy.addWidget(self._name)
        copy.addWidget(self._state)
        row.addLayout(copy, 1)

    def set_document(self, document_id: str, filename: str, state: str, *, busy: bool) -> None:
        self._document_id = document_id
        self._busy = busy
        self._name.setText(filename)
        self._state.setText(state)
        self.setToolTip(f"{filename} vẫn đang xử lý" if busy else f"Xem nội dung {filename}")
        self.setCursor(
            Qt.CursorShape.ForbiddenCursor if busy else Qt.CursorShape.PointingHandCursor
        )

    def mousePressEvent(self, event) -> None:  # noqa: N802 - Qt override
        if not self._busy and self._document_id:
            self.activated.emit(self._document_id)
        super().mousePressEvent(event)


class _MessageBubble(QFrame):
    copyRequested = Signal(str)
    regenerateRequested = Signal()
    citationActivated = Signal(str)

    def __init__(self, role: str, author: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.role = role
        self._content = ""
        self.setProperty("class", "card" if role == "assistant" else "panel")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(*theme.CARD_MARGINS)
        layout.setSpacing(theme.CARD_SPACING)

        header = QHBoxLayout()
        header.setSpacing(theme.SPACE["sm"])
        avatar = QLabel("AI" if role == "assistant" else _initials(author))
        avatar.setProperty("class", "avatar")
        avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        name = QLabel("Private AI" if role == "assistant" else author)
        name.setProperty("class", "section-label")
        header.addWidget(avatar)
        header.addWidget(name)
        header.addStretch(1)
        layout.addLayout(header)

        self._status = QLabel(THINKING)
        self._status.setProperty("class", "muted")
        self._status.hide()
        layout.addWidget(self._status)

        # Where the answer will be. Until a token arrives this is the only thing saying
        # the turn is alive, so it stands in the answer's place rather than beside it.
        self._trail = ReasoningTrail(self)
        self._trail.hide()
        layout.addWidget(self._trail)

        self._body = _FittedBrowser()
        self._body.document().setDefaultStyleSheet(md.document_css(theme.tokens()))
        layout.addWidget(self._body)

        # How the sources were found. With ``auto`` the routing is a decision the user
        # never made and cannot see anywhere else, and it is the answer to "why did it
        # cite these and not those" long after the trail has collapsed.
        self._retrieval = QLabel()
        self._retrieval.setWordWrap(True)
        self._retrieval.setProperty("class", "faint")
        self._retrieval.hide()
        layout.addWidget(self._retrieval)

        self._citations = QWidget()
        self._citations_layout = QHBoxLayout(self._citations)
        self._citations_layout.setContentsMargins(0, theme.SPACE["3xs"], 0, 0)
        self._citations_layout.setSpacing(theme.SPACE["xs"])
        self._citations.hide()
        layout.addWidget(self._citations)

        self._actions = QWidget()
        actions = QHBoxLayout(self._actions)
        actions.setContentsMargins(0, theme.SPACE["3xs"], 0, 0)
        actions.setSpacing(theme.SPACE["xs"])
        self._copy = QToolButton()
        self._copy.setIcon(icon("copy", size=15))
        self._copy.setText("Sao chép")
        self._copy.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self._copy.setToolTip("Sao chép câu trả lời")
        self._copy.clicked.connect(lambda: self.copyRequested.emit(self._content))
        self._regenerate = QToolButton()
        self._regenerate.setIcon(icon("refresh-cw", size=15))
        self._regenerate.setText("Tạo lại")
        self._regenerate.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self._regenerate.setToolTip("Tạo lại câu trả lời")
        self._regenerate.clicked.connect(self.regenerateRequested)
        actions.addWidget(self._copy)
        actions.addWidget(self._regenerate)
        actions.addStretch(1)
        self._actions.hide()
        layout.addWidget(self._actions)

    def content(self) -> str:
        return self._content

    def apply_style(self, css: str) -> None:
        self._body.document().setDefaultStyleSheet(css)
        self.set_content(self._content)

    def set_content(self, text: str) -> None:
        self._content = text
        if self.role == "assistant":
            self._body.setHtml(md.markdown_to_html(text))
        else:
            self._body.setPlainText(text)
        self._body.setVisible(bool(text))
        idle = not text and self.role == "assistant"
        self._status.setVisible(idle and not self._trail.isVisible())

    def set_status(self, text: str) -> None:
        self._status.setText(text)
        self._status.setVisible(bool(text) and not self._content and not self._trail.isVisible())

    def trail(self) -> ReasoningTrail:
        """The progress trail. The view drives it; the bubble only owns where it sits."""
        return self._trail

    def begin_progress(self, label: str) -> None:
        self._trail.start(label)
        self._trail.show()
        self._status.hide()

    def end_progress(self, label: str = "") -> None:
        """Collapse the trail to its summary line, or to nothing.

        A turn that took a couple of seconds never looked like a hang, so it leaves
        nothing behind; a slow one, or one that degraded, keeps a line saying so.
        """
        self._trail.finish(label)
        self._trail.setVisible(self._trail.has_content())

    def set_error(self, message: str) -> None:
        """Shown beneath whatever partial answer arrived, never instead of it."""
        self._status.setText(message)
        self._status.setProperty("class", "danger")
        self._status.setVisible(bool(message))
        theme.restyle(self._status)

    def set_actions_visible(self, visible: bool, *, can_regenerate: bool = True) -> None:
        self._regenerate.setVisible(can_regenerate)
        self._actions.setVisible(visible and bool(self._content))

    def set_retrieval(self, strategy: str, routed_to: str, reason: str) -> None:
        """Say which strategy produced the sources, and — when a router chose — why."""
        used = routed_to or strategy
        if not used:
            self._retrieval.hide()
            return
        text = f"Tìm bằng {strategy_label(used)}"
        if routed_to and strategy != routed_to and reason:
            text = f"{text} (tự chọn: {reason})"
        self._retrieval.setText(text)
        self._retrieval.show()

    def set_citations(self, citations: Sequence[dict[str, Any]]) -> None:
        while self._citations_layout.count():
            item = self._citations_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        seen: set[str] = set()
        shown = 0
        for citation in citations:
            document_id = str(citation.get("document_id") or "")
            if not document_id or document_id in seen:
                continue
            seen.add(document_id)
            label = str(citation.get("filename") or "Nguồn")
            page = citation.get("page")
            if page:
                label = f"{label} · tr. {page}"
            chip = QToolButton()
            chip.setProperty("class", "chip")
            chip.setText(label)
            chip.setIcon(icon("file-text", size=14))
            chip.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
            chip.setToolTip(str(citation.get("snippet") or label))
            chip.clicked.connect(
                lambda _checked=False, i=document_id: self.citationActivated.emit(i)
            )
            self._citations_layout.addWidget(chip)
            shown += 1
        self._citations_layout.addStretch(1)
        self._citations.setVisible(bool(shown))


class _Composer(QPlainTextEdit):
    """Enter sends, Shift+Enter breaks the line — the one keyboard rule people expect."""

    submitted = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._composing = False

    def inputMethodEvent(self, event: Any) -> None:  # noqa: N802
        # Vietnamese input goes through an IME, and Enter there commits the candidate
        # rather than ending the sentence. Sending mid-composition would eat the word.
        self._composing = bool(event.preeditString())
        super().inputMethodEvent(event)

    def keyPressEvent(self, event: Any) -> None:  # noqa: N802
        enter = event.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter)
        shift = bool(event.modifiers() & Qt.KeyboardModifier.ShiftModifier)
        if enter and not shift and not self._composing:
            event.accept()
            self.submitted.emit()
            return
        super().keyPressEvent(event)


class ChatView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.ctx = ctx
        self._messages: list[dict[str, Any]] = []
        self._bubbles: list[_MessageBubble] = []
        self._conversation_id = ctx.conversation_id
        self._profile_name = ""
        self._documents: list[dict[str, Any]] = []
        self._document_total = 0
        self._has_workspace = False
        self._workspace_name = ""

        self._stream_task: asyncio.Task[None] | None = None
        self._stream_bubble: _MessageBubble | None = None
        self._stream_text = ""
        self._stream_dirty = False
        self._follow = True

        self._repaint = QTimer(self)
        self._repaint.setInterval(REPAINT_INTERVAL_MS)
        self._repaint.timeout.connect(self._flush_tokens)
        self._poll = QTimer(self)
        self._poll.setInterval(CONTEXT_POLL_MS)
        self._poll.timeout.connect(self._poll_context)

        self._capture = MicrophoneCapture(ctx.services, parent=self)
        self._capture.transcriptChanged.connect(self._on_transcript)
        self._capture.stateChanged.connect(self._on_voice_state)
        self._capture.failed.connect(self._set_error)

        self.setAcceptDrops(True)
        self._build()
        self._wire()
        self._apply_preferences(ctx.preferences)
        self._rebuild_messages()
        # Deferred: the qasync loop is not running yet while MainWindow is being built.
        QTimer.singleShot(0, self.refresh)

    # --- construction -----------------------------------------------------

    def _build(self) -> None:
        root = QHBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)
        root.addWidget(self._build_conversation(), 1)
        root.addWidget(self._build_rail(), 0)

    def _build_conversation(self) -> QWidget:
        column = QWidget()
        layout = QVBoxLayout(column)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        strip = QHBoxLayout()
        strip.setContentsMargins(theme.SPACE["2xl"], theme.SPACE["sm"], theme.SPACE["md"], 0)
        strip.setSpacing(theme.TOOLBAR_SPACING)
        strip.addStretch(1)
        self._rail_toggle = QToolButton()
        self._rail_toggle.setCheckable(True)
        self._rail_toggle.setChecked(True)
        self._rail_toggle.setIcon(icon("panel-right-close"))
        describe(self._rail_toggle, "Ẩn bảng ngữ cảnh")
        self._rail_toggle.toggled.connect(self._toggle_rail)
        strip.addWidget(self._rail_toggle)
        layout.addLayout(strip)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._stream_host = QWidget()
        self._stream_layout = QVBoxLayout(self._stream_host)
        self._stream_layout.setContentsMargins(*theme.PAGE_MARGINS)
        self._stream_layout.setSpacing(theme.SPACE["md"])
        self._stream_layout.addStretch(1)
        self._scroll.setWidget(self._stream_host)
        layout.addWidget(self._scroll, 1)

        self._scroll_button = QToolButton(self._scroll)
        self._scroll_button.setIcon(icon("chevron-down", size=16))
        self._scroll_button.setText(SCROLL_LATEST)
        self._scroll_button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self._scroll_button.setProperty("class", "chip")
        self._scroll_button.clicked.connect(self._scroll_to_bottom)
        self._scroll_button.hide()

        layout.addWidget(self._build_composer())
        return column

    def _build_composer(self) -> QWidget:
        wrap = QWidget()
        layout = QVBoxLayout(wrap)
        layout.setContentsMargins(theme.SPACE["2xl"], 0, theme.SPACE["2xl"], theme.SPACE["lg"])
        layout.setSpacing(theme.SPACE["xs"])

        self._error = QLabel()
        self._error.setProperty("class", "danger")
        self._error.setWordWrap(True)
        self._error.hide()
        layout.addWidget(self._error)

        frame = QFrame()
        frame.setProperty("class", "card")
        inner = QVBoxLayout(frame)
        inner.setContentsMargins(*theme.CARD_MARGINS)
        inner.setSpacing(theme.SPACE["sm"])

        self._composer = _Composer()
        self._composer.setPlaceholderText("Nhập câu hỏi cho Private AI…")
        self._composer.setFixedHeight(_COMPOSER_HEIGHT)
        # Otherwise a dropped file lands as its own path, pasted into the message.
        self._composer.setAcceptDrops(False)
        self._composer.setFrameShape(QFrame.Shape.NoFrame)
        self._composer.submitted.connect(self._on_submit)
        inner.addWidget(self._composer)

        tools = QHBoxLayout()
        tools.setSpacing(theme.TOOLBAR_SPACING)
        self._model_picker = ModelPicker()
        self._model_picker.set_placeholder("Chọn mô hình")
        tools.addWidget(self._model_picker)

        self._strategy = StrategyPicker()
        tools.addWidget(self._strategy)

        self._web = QToolButton()
        self._web.setCheckable(True)
        self._web.setIcon(icon("globe"))
        # Named at build time as well as on every preference change: the tooltip used to
        # arrive only with the first async read, so the control was nameless until then.
        describe(self._web, "Tra cứu trên web trước khi trả lời. Câu hỏi sẽ rời khỏi máy này.")
        tools.addWidget(self._web)

        self._attach = QToolButton()
        self._attach.setIcon(icon("paperclip"))
        describe(self._attach, "Đính kèm tài liệu")
        tools.addWidget(self._attach)

        self._mic = QToolButton()
        self._mic.setIcon(icon("mic"))
        describe(self._mic, "Ghi âm câu hỏi")
        tools.addWidget(self._mic)

        tools.addStretch(1)
        self._send = QToolButton()
        self._send.setIcon(icon("chevron-right"))
        describe(self._send, "Gửi tin nhắn")
        tools.addWidget(self._send)
        inner.addLayout(tools)
        layout.addWidget(frame)

        hint = QLabel("Enter để gửi · Shift + Enter để xuống dòng")
        hint.setProperty("class", "muted")
        layout.addWidget(hint)
        return wrap

    def _build_rail(self) -> QWidget:
        self._rail = QFrame()
        self._rail.setObjectName("ContextRail")
        self._rail.setFixedWidth(RAIL_WIDTH)
        layout = QVBoxLayout(self._rail)
        layout.setContentsMargins(*theme.CARD_MARGINS)
        layout.setSpacing(theme.SPACE["sm"])

        heading = QLabel("Ngữ cảnh")
        heading.setProperty("class", "section-label")
        layout.addWidget(heading)
        self._rail_workspace = QLabel("Chưa có không gian")
        self._rail_workspace.setProperty("class", "heading")
        self._rail_workspace.setWordWrap(True)
        layout.addWidget(self._rail_workspace)

        documents_label = QLabel("Tài liệu")
        documents_label.setProperty("class", "section-label")
        layout.addWidget(documents_label)

        self._document_rows: list[tuple[_RailDocument, QProgressBar]] = []
        for _slot in range(RAIL_DOCUMENTS):
            row = _RailDocument()
            bar = QProgressBar()
            bar.setRange(0, 100)
            bar.setTextVisible(False)
            # Connected once: reconnecting per repaint made PySide warn about
            # disconnecting a never-connected signal on the first paint. The row's
            # current document is carried on the row instead.
            row.activated.connect(self._open_document)
            layout.addWidget(row)
            layout.addWidget(bar)
            row.hide()
            bar.hide()
            self._document_rows.append((row, bar))

        self._documents_empty = QLabel("Chưa có tài liệu trong không gian này.")
        self._documents_empty.setProperty("class", "muted")
        self._documents_empty.setWordWrap(True)
        layout.addWidget(self._documents_empty)

        self._view_all = QToolButton()
        self._view_all.setText("Xem toàn bộ tài liệu")
        self._view_all.clicked.connect(lambda: self.ctx.navigate("library"))
        self._view_all.hide()
        layout.addWidget(self._view_all)

        self._rail_add = QToolButton()
        self._rail_add.setIcon(icon("upload", size=16))
        self._rail_add.setText("Thêm tài liệu")
        self._rail_add.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self._rail_add.clicked.connect(lambda: self._open_upload([]))
        layout.addWidget(self._rail_add)

        status_label = QLabel("Trạng thái hệ thống")
        status_label.setProperty("class", "section-label")
        layout.addWidget(status_label)
        self._pips: dict[str, tuple[StatusPipLabel, QLabel]] = {}
        for key, caption in (
            ("provider", "Nhà cung cấp AI"),
            ("knowledge_graph", "Kho tri thức"),
            ("asr", "Giọng nói"),
        ):
            # Stacked, not two columns. The rail is 296px wide and these values are
            # sentences ("Ollama cục bộ · trên thiết bị"); squeezed into the half-width
            # right column they wrapped into a ragged block hanging off the caption.
            entry = QVBoxLayout()
            entry.setSpacing(theme.SPACE["3xs"])
            pip = StatusPipLabel(caption, "unknown")
            value = QLabel("Đang kiểm tra…")
            value.setProperty("class", "muted")
            value.setWordWrap(True)
            # Indented onto the caption's own text column, past the pip beside it.
            value.setContentsMargins(STATUS_TEXT_INSET, 0, 0, 0)
            entry.addWidget(pip)
            entry.addWidget(value)
            layout.addLayout(entry)
            self._pips[key] = (pip, value)

        self._vram_title = QLabel("VRAM đang dùng")
        self._vram_title.setProperty("class", "section-label")
        layout.addWidget(self._vram_title)
        self._vram_value = QLabel("Đang đo…")
        self._vram_value.setProperty("class", "muted")
        layout.addWidget(self._vram_value)
        self._vram_bar = QProgressBar()
        self._vram_bar.setRange(0, 100)
        self._vram_bar.setTextVisible(False)
        layout.addWidget(self._vram_bar)
        self._vram_detail = QLabel("")
        self._vram_detail.setProperty("class", "muted")
        self._vram_detail.setWordWrap(True)
        layout.addWidget(self._vram_detail)

        layout.addStretch(1)

        note = QFrame()
        note.setProperty("class", "panel")
        note_layout = QVBoxLayout(note)
        note_layout.setContentsMargins(*theme.CARD_MARGINS)
        note_layout.setSpacing(theme.SPACE["2xs"])
        self._privacy_title = QLabel(ON_DEVICE_TITLE)
        self._privacy_title.setProperty("class", "section-label")
        # Where the conversation goes is not tertiary text, however small the panel is.
        self._privacy_body = QLabel(ON_DEVICE_BODY)
        self._privacy_body.setProperty("class", "muted")
        self._privacy_body.setWordWrap(True)
        note_layout.addWidget(self._privacy_title)
        note_layout.addWidget(self._privacy_body)
        layout.addWidget(note)
        return self._rail

    def _wire(self) -> None:
        ctx = self.ctx
        ctx.workspaceChanged.connect(self._on_workspace_changed)
        ctx.conversationChanged.connect(self._on_conversation_changed)
        ctx.documentsChanged.connect(self.refresh)
        ctx.modelsChanged.connect(self._load_models)
        ctx.preferencesChanged.connect(self._apply_preferences)
        ctx.themeChanged.connect(self._on_theme_changed)
        self._send.clicked.connect(self._on_send_clicked)
        self._attach.clicked.connect(lambda: self._open_upload([]))
        self._mic.clicked.connect(self._on_mic_clicked)
        self._web.toggled.connect(self._on_web_toggled)
        self._model_picker.selectionChanged.connect(self._on_model_selected)
        self._model_picker.manageRequested.connect(lambda: self.ctx.navigate("settings", "models"))
        self._scroll.verticalScrollBar().valueChanged.connect(self._on_scrolled)

    # --- lifecycle --------------------------------------------------------

    def on_activated(self) -> None:
        self._poll.start()
        self.refresh()

    def on_deactivated(self) -> None:
        self._poll.stop()

    def resizeEvent(self, event: QResizeEvent) -> None:  # noqa: N802
        super().resizeEvent(event)
        self._place_scroll_button()

    def _place_scroll_button(self) -> None:
        button = getattr(self, "_scroll_button", None)
        if button is None:
            return
        button.adjustSize()
        area = self._scroll.viewport().size()
        button.move(
            max(0, (area.width() - button.width()) // 2),
            max(0, area.height() - button.height() - theme.SPACE["md"]),
        )

    # --- preferences ------------------------------------------------------

    def _apply_preferences(self, preferences: Any) -> None:
        with contextlib.suppress(RuntimeError):
            saved = str(preferences.retrieval_strategy)
            web = bool(preferences.web_search_enabled)
            if saved == RetrievalStrategyName.WEB.value:
                # Web used to be one of the strategies *and* the globe toggle. The toggle
                # won, so a default saved under the old meaning moves onto it once, rather
                # than silently becoming "auto, with web off".
                saved = RetrievalStrategyName.AUTO.value
                web = True
                self._persist(RETRIEVAL_STRATEGY_KEY, saved)
                self._persist(WEB_SEARCH_ENABLED_KEY, "1")
            self._strategy.set_default(saved)
            self._web.blockSignals(True)
            self._web.setChecked(web)
            self._web.blockSignals(False)
        self._refresh_web_button()

    def _on_web_toggled(self, enabled: bool) -> None:
        self._refresh_web_button()
        self._persist(WEB_SEARCH_ENABLED_KEY, "1" if enabled else "0")

    def _refresh_web_button(self) -> None:
        enabled = self._web.isChecked()
        self._web.setText(
            str(self.ctx.preferences.web_search_backend) if enabled else "",
        )
        self._web.setToolButtonStyle(
            Qt.ToolButtonStyle.ToolButtonTextBesideIcon
            if enabled
            else Qt.ToolButtonStyle.ToolButtonIconOnly
        )
        describe(
            self._web,
            "Câu hỏi sẽ được gửi tới nguồn tìm kiếm đã chọn trong Cài đặt"
            if enabled
            else "Tra cứu trên web trước khi trả lời. Câu hỏi sẽ rời khỏi máy này.",
        )

    def _persist(self, key: str, value: str) -> None:
        """Optimistic: the control has already moved, so only a failure is reported."""
        self.ctx.run(
            write_app_preference_async(self.ctx.database, key, value),
            on_result=lambda _result: self.ctx.refresh_preferences(),
            on_error=self._on_persist_failed,
        )

    def _on_persist_failed(self, exc: BaseException) -> None:
        self.ctx.toast(f"Không lưu được tuỳ chọn: {exc}", "error")
        self._apply_preferences(self.ctx.preferences)

    def _on_theme_changed(self, _name: str) -> None:
        style = md.document_css(theme.tokens())
        for bubble in self._bubbles:
            bubble.apply_style(style)

    # --- messages ---------------------------------------------------------

    def _on_workspace_changed(self, _workspace_id: str) -> None:
        self._messages = []
        self._conversation_id = ""
        self._rebuild_messages()
        self.refresh()

    def _on_conversation_changed(self, conversation_id: str) -> None:
        if conversation_id == self._conversation_id:
            return
        self._conversation_id = conversation_id
        if not conversation_id:
            self._messages = []
            self._rebuild_messages()
            return
        self.ctx.run(
            repositories.get_conversation(self.ctx.database, conversation_id),
            on_result=self._load_messages,
            on_error=lambda exc: self._set_error(f"Không mở được cuộc trò chuyện: {exc}"),
        )

    def _load_messages(self, conversation: Any) -> None:
        self._messages = [
            {"role": message.role, "content": message.content, "citations": []}
            for message in conversation.messages
            if message.role in ("user", "assistant")
        ]
        self._rebuild_messages()
        self._follow = True
        self._scroll_to_bottom()

    def _clear_area(self) -> None:
        while self._stream_layout.count() > 1:
            item = self._stream_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        self._bubbles = []

    def _rebuild_messages(self) -> None:
        self._clear_area()
        if not self._messages:
            self._stream_layout.insertWidget(0, self._welcome())
            return
        for message in self._messages:
            self._add_bubble(message)
        self._refresh_actions()

    def _add_bubble(self, message: dict[str, Any]) -> _MessageBubble:
        bubble = _MessageBubble(str(message["role"]), self._profile_name or "Bạn")
        bubble.copyRequested.connect(self._copy)
        bubble.regenerateRequested.connect(self._regenerate)
        bubble.citationActivated.connect(self._open_document)
        bubble.set_content(str(message.get("content") or ""))
        bubble.set_citations(message.get("citations") or [])
        self._stream_layout.insertWidget(self._stream_layout.count() - 1, bubble)
        self._bubbles.append(bubble)
        return bubble

    def _refresh_actions(self) -> None:
        """Copy on every answer; Regenerate only on the last, and only once it is done."""
        last = len(self._bubbles) - 1
        for position, bubble in enumerate(self._bubbles):
            if bubble.role != "assistant":
                bubble.set_actions_visible(False)
                continue
            bubble.set_actions_visible(
                True,
                can_regenerate=position == last and self._stream_task is None,
            )

    def _welcome(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(theme.SPACE["sm"])
        greeting = QLabel(f"Chào bạn, {self._profile_name}" if self._profile_name else "Chào bạn")
        greeting.setProperty("class", "muted")
        title = QLabel("Hôm nay bạn muốn làm gì?")
        title.setProperty("class", "title")
        blurb = QLabel(
            "Hỏi bằng ngôn ngữ tự nhiên. Private AI sẽ dùng mô hình và tài liệu "
            "trên máy để trả lời."
        )
        blurb.setProperty("class", "muted")
        blurb.setWordWrap(True)
        layout.addWidget(greeting)
        layout.addWidget(title)
        layout.addWidget(blurb)

        if not self._has_workspace or not self._model():
            card = QFrame()
            card.setProperty("class", "card")
            box = QVBoxLayout(card)
            box.setContentsMargins(*theme.CARD_MARGINS)
            box.setSpacing(theme.CARD_SPACING)
            headline = QLabel("Thiết lập trước khi trò chuyện")
            headline.setProperty("class", "heading")
            detail = QLabel("Tạo nơi lưu dữ liệu và chọn một mô hình có thể trả lời.")
            detail.setProperty("class", "muted")
            detail.setWordWrap(True)
            box.addWidget(headline)
            box.addWidget(detail)
            row = QHBoxLayout()
            row.setSpacing(theme.TOOLBAR_SPACING)
            if not self._has_workspace:
                create = QToolButton()
                create.setIcon(icon("plus", size=16))
                create.setText("Tạo không gian")
                create.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
                create.clicked.connect(lambda: self.ctx.navigate("workspaces"))
                row.addWidget(create)
            if not self._model():
                choose = QToolButton()
                choose.setIcon(icon("boxes", size=16))
                choose.setText("Chọn mô hình")
                choose.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
                choose.clicked.connect(lambda: self.ctx.navigate("settings", "models"))
                row.addWidget(choose)
            row.addStretch(1)
            box.addLayout(row)
            layout.addWidget(card)
        else:
            for prompt in STARTER_PROMPTS:
                starter = QToolButton()
                starter.setText(prompt)
                starter.setProperty("class", "chip")
                # A chip hugs its text; stretched down the column it reads as a button bar.
                starter.clicked.connect(lambda _checked=False, p=prompt: self._submit(p))
                layout.addWidget(starter, 0, Qt.AlignmentFlag.AlignLeft)
        return page

    # --- sending ----------------------------------------------------------

    def _model(self) -> str:
        return self._model_picker.current()

    def _on_submit(self) -> None:
        self._submit(self._composer.toPlainText())

    def _on_send_clicked(self) -> None:
        if self._stream_task is not None:
            self._stop()
            return
        self._on_submit()

    def _stop(self) -> None:
        task = self._stream_task
        if task is not None and not task.done():
            # The runner writes the partial answer down in its own finally, so the bubble
            # on screen and the row in the database stay in agreement.
            task.cancel()

    def _submit(self, content: str) -> None:
        text = content.strip()
        if not text or self._stream_task is not None:
            return
        if not self._model():
            self._set_error(NEED_MODEL)
            return
        if not self.ctx.workspace_id:
            self._set_error(NEED_WORKSPACE)
            return
        self._set_error("")
        self._follow = self._near_bottom()
        self._composer.clear()
        if not self._messages:
            # Drop the welcome card; _rebuild_messages would only put it straight back.
            self._clear_area()
        self._messages.append({"role": "user", "content": text, "citations": []})
        self._add_bubble(self._messages[-1])
        self._messages.append({"role": "assistant", "content": "", "citations": []})
        self._stream_bubble = self._add_bubble(self._messages[-1])
        # The clock starts with the turn, not with the first progress event: the gap
        # before the graph reports anything is itself part of the wait.
        self._stream_bubble.begin_progress(THINKING)
        self._refresh_actions()
        self._stream_text = ""
        self._stream_dirty = False
        self._set_running(True)
        self._repaint.start()
        self._scroll_to_bottom()
        self._stream_task = self.ctx.run(self._turn(text), on_error=self._on_turn_failed)

    async def _turn(self, content: str) -> None:
        ctx = self.ctx
        try:
            conversation_id = self._conversation_id or ctx.conversation_id
            if not conversation_id:
                conversation = await repositories.create_conversation(
                    ctx.database,
                    ctx.workspace_id,
                    ConversationCreate(model=self._model()),
                )
                conversation_id = conversation.id
                self._conversation_id = conversation_id
                ctx.set_conversation(conversation_id)
            agent = ctx.services.agent
            if agent is None:
                self._on_stream_error("Trợ lý chưa sẵn sàng")
                return
            async for event in agent.stream(
                conversation_id=conversation_id,
                content=content,
                workspace_id=ctx.workspace_id,
                model=self._model(),
                strategy=self._strategy.current(),
                web_search=self._web.isChecked(),
            ):
                self._handle_event(event)
        finally:
            self._finish_turn()

    def _handle_event(self, event: dict[str, Any]) -> None:
        kind = str(event.get("type") or "")
        if kind == "token":
            self._stream_text += str(event.get("content") or "")
            self._stream_dirty = True
        elif kind == "progress":
            self._on_progress(event)
        elif kind == "tool_start":
            self._set_tool(str(event.get("name") or ""))
        elif kind == "tool_end":
            self._set_tool("")
        elif kind == "notice":
            # A degraded sub-service, not a failed turn: say so and keep streaming. It
            # goes into the trail as well, because a toast is gone in a few seconds and
            # "why is there no citation" is asked long after that.
            message = str(event.get("message") or "")
            bubble = self._stream_bubble
            if bubble is not None:
                bubble.trail().note(message)
            self.ctx.toast(message, "info")
        elif kind == "error":
            self._on_stream_error(str(event.get("message") or ""))
        elif kind == "final":
            self._on_final(event)

    def _on_progress(self, event: dict[str, Any]) -> None:
        bubble = self._stream_bubble
        if bubble is None:
            return
        label = str(event.get("label") or "").strip()
        if not label:
            return
        detail = str(event.get("detail") or "").strip()
        fraction = event.get("fraction")
        if isinstance(fraction, int | float) and 0.0 <= float(fraction) <= 1.0:
            percent = f"{round(float(fraction) * 100)}%"
            detail = f"{detail} · {percent}" if detail else percent
        bubble.trail().step(label, detail)

    def _set_tool(self, name: str) -> None:
        bubble = self._stream_bubble
        if bubble is None:
            return
        # A finished tool hands the turn straight back to the model, so say that rather
        # than dropping to a generic "thinking" that reads like the step was lost.
        bubble.trail().step(f"Đang dùng {name}" if name else "Soạn câu trả lời")

    def _flush_tokens(self) -> None:
        if not self._stream_dirty:
            return
        self._stream_dirty = False
        bubble = self._stream_bubble
        if bubble is None:
            return
        # Once text is arriving the path taken has done its job; the trail collapses to
        # its running line so it cannot push the answer down the bubble.
        bubble.trail().collapse()
        bubble.set_content(self._stream_text)
        if self._messages:
            self._messages[-1]["content"] = self._stream_text
        self._after_grow()

    def _on_final(self, event: dict[str, Any]) -> None:
        self._stream_text = str(event.get("content") or self._stream_text)
        self._stream_dirty = False
        citations = [
            fields
            for fields in (_citation_fields(item) for item in event.get("citations") or [])
            if fields
        ]
        if self._messages:
            self._messages[-1] = {
                "role": "assistant",
                "content": self._stream_text,
                "citations": citations,
            }
        bubble = self._stream_bubble
        if bubble is not None:
            bubble.set_content(self._stream_text)
            bubble.set_retrieval(
                str(event.get("strategy") or ""),
                str(event.get("routed_to") or ""),
                str(event.get("routing_reason") or ""),
            )
            bubble.set_citations(citations)
        self._after_grow()

    def _on_stream_error(self, message: str) -> None:
        self._set_error(message)
        bubble = self._stream_bubble
        if bubble is not None:
            bubble.trail().fail(message)
            bubble.set_error(message)

    def _on_turn_failed(self, exc: BaseException) -> None:
        self._on_stream_error(str(exc) or "Không thể gửi tin nhắn")

    def _finish_turn(self) -> None:
        self._repaint.stop()
        self._stream_dirty = True
        self._flush_tokens()
        bubble = self._stream_bubble
        if bubble is not None:
            bubble.end_progress()
        self._stream_task = None
        self._stream_bubble = None
        self._set_running(False)
        self._refresh_actions()
        self.refresh()

    def _set_running(self, running: bool) -> None:
        self._send.setIcon(icon("stop-circle" if running else "chevron-right"))
        describe(self._send, "Dừng trả lời" if running else "Gửi tin nhắn")
        self._strategy.setEnabled(not running)
        self._web.setEnabled(not running)
        self._mic.setEnabled(not running and not self._capture.is_busy())

    # --- per-message actions ----------------------------------------------

    def _copy(self, content: str) -> None:
        clipboard = QGuiApplication.clipboard()
        if clipboard is None:
            self.ctx.toast("Không truy cập được bộ nhớ tạm", "error")
            return
        clipboard.setText(content)
        self.ctx.toast("Đã sao chép câu trả lời", "success")

    def _regenerate(self) -> None:
        if self._stream_task is not None:
            return
        index = len(self._messages) - 1
        prompt = ""
        while index >= 0:
            if self._messages[index]["role"] == "user":
                prompt = str(self._messages[index]["content"])
                break
            index -= 1
        if not prompt:
            return
        self._messages = self._messages[:index]
        self._rebuild_messages()
        self._submit(prompt)

    # --- scrolling --------------------------------------------------------

    def _near_bottom(self) -> bool:
        bar = self._scroll.verticalScrollBar()
        return bar.maximum() - bar.value() < NEAR_BOTTOM_PX

    def _on_scrolled(self, _value: int) -> None:
        self._follow = self._near_bottom()
        self._scroll_button.setVisible(not self._follow and bool(self._messages))
        self._place_scroll_button()

    def _after_grow(self) -> None:
        if self._follow:
            self._scroll_to_bottom()
            return
        self._scroll_button.show()
        self._place_scroll_button()

    def _scroll_to_bottom(self) -> None:
        self._follow = True
        self._scroll_button.hide()
        # Deferred: the bubble that just grew has not been laid out yet, so the
        # scrollbar's maximum is still the old one.
        QTimer.singleShot(0, self._snap_to_bottom)

    def _snap_to_bottom(self) -> None:
        bar = self._scroll.verticalScrollBar()
        bar.setValue(bar.maximum())

    # --- voice ------------------------------------------------------------

    def _on_mic_clicked(self) -> None:
        if self._capture.is_recording():
            self._capture.stop()
            return
        if self._capture.is_busy():
            return
        self._set_error("")
        self.ctx.run(self._capture.start(self._composer.toPlainText()))

    def _on_transcript(self, text: str) -> None:
        self._composer.setPlainText(text)
        cursor = self._composer.textCursor()
        cursor.movePosition(QTextCursor.MoveOperation.End)
        self._composer.setTextCursor(cursor)

    def _on_voice_state(self, state: str) -> None:
        recording = state == STATE_RECORDING
        self._mic.setIcon(icon("mic-off" if recording else "mic"))
        describe(
            self._mic,
            "Dừng ghi âm"
            if recording
            else "Đang nhận dạng giọng nói"
            if state == STATE_TRANSCRIBING
            else "Ghi âm câu hỏi",
        )
        self._mic.setEnabled(state != STATE_TRANSCRIBING and self._stream_task is None)

    # --- attachments ------------------------------------------------------

    def dragEnterEvent(self, event: QDragEnterEvent) -> None:  # noqa: N802
        # Qt tracks enter/leave per widget, so the DOM's drag-depth counter has no
        # equivalent here and none is needed.
        if accepted_paths(event.mimeData()):
            event.acceptProposedAction()

    def dropEvent(self, event: QDropEvent) -> None:  # noqa: N802
        paths = accepted_paths(event.mimeData())
        if not paths:
            return
        event.acceptProposedAction()
        self._open_upload(paths)

    def _open_upload(self, paths: Sequence[Any]) -> None:
        dialog = UploadDialog(
            self.ctx,
            workspace_id=self.ctx.workspace_id,
            workspace_name=self._workspace_name,
            files=list(paths),
            parent=self,
        )
        dialog.completed.connect(lambda _result: self.refresh())
        dialog.show()

    def _open_document(self, document_id: str) -> None:
        if document_id:
            DocumentViewer(self.ctx, document_id, parent=self).show()

    # --- context rail -----------------------------------------------------

    def refresh(self) -> None:
        self.ctx.run(self._load_context(), on_error=lambda exc: self._set_error(str(exc)))
        self._load_models()

    def _poll_context(self) -> None:
        """Poll only while something is moving; an idle chat costs no queries."""
        if self._stream_task is not None or any(is_document_busy(d) for d in self._documents):
            self.refresh()

    async def _load_context(self) -> None:
        ctx = self.ctx
        database = ctx.database
        workspace_id = ctx.workspace_id

        profile_id = await repositories.active_profile_id_async(database)
        if profile_id:
            with contextlib.suppress(repositories.NotFound):
                profile = await repositories.get_profile(database, profile_id)
                self._profile_name = profile.display_name

        self._has_workspace = False
        self._workspace_name = ""
        self._documents = []
        self._document_total = 0
        if workspace_id:
            with contextlib.suppress(repositories.NotFound):
                workspace = await repositories.get_workspace(database, workspace_id)
                self._has_workspace = True
                self._workspace_name = workspace.name
            if self._has_workspace:
                page = await repositories.list_documents(
                    database,
                    workspace_id,
                    limit=RAIL_DOCUMENTS,
                )
                self._documents = list(page["items"])
                self._document_total = int(page["total"])

        self._paint_documents()
        await self._paint_status()
        self._paint_vram()
        if not self._messages:
            self._rebuild_messages()

    def _paint_documents(self) -> None:
        self._rail_workspace.setText(self._workspace_name or "Chưa có không gian")
        for slot, (button, bar) in enumerate(self._document_rows):
            if slot >= len(self._documents):
                button.hide()
                bar.hide()
                continue
            document = self._documents[slot]
            busy = is_document_busy(document)
            document_id = str(document.get("id") or "")
            filename = str(document.get("filename") or "")
            button.set_document(
                document_id,
                filename,
                document_status_text(document),
                busy=busy,
            )
            button.show()
            bar.setValue(int(round(document_progress(document) * 100)))
            bar.setVisible(busy)
        self._documents_empty.setVisible(self._has_workspace and not self._documents)
        self._view_all.setVisible(self._document_total > RAIL_DOCUMENTS)
        self._view_all.setText(f"Xem toàn bộ {self._document_total} tài liệu")
        self._rail_add.setEnabled(self._has_workspace)

    async def _paint_status(self) -> None:
        services = self.ctx.services
        provider = services.providers.active_config()
        online = await services.models.health()
        pip, value = self._pips["provider"]
        if provider is None:
            pip.set_state("not_configured")
            value.setText("Chưa cấu hình")
            on_device = True
        else:
            on_device = provider.on_device
            pip.set_state("online" if online else "offline")
            if online:
                value.setText(f"{provider.name} · {'trên thiết bị' if on_device else 'từ xa'}")
            else:
                value.setText(f"{provider.name} · không phản hồi")

        graph_ready = await services.graph.health()
        pip, value = self._pips["knowledge_graph"]
        pip.set_state("online" if graph_ready else "not_configured")
        value.setText("Sẵn sàng" if graph_ready else "Chưa dựng")

        asr_ready = await services.asr.health()
        can_record = asr_ready and MicrophoneCapture.available()
        pip, value = self._pips["asr"]
        pip.set_state("online" if can_record else "not_configured")
        value.setText("Sẵn sàng" if can_record else "Chưa cấu hình")
        self._mic.setEnabled(can_record and self._stream_task is None)
        if not can_record:
            describe(self._mic, "Giọng nói chưa sẵn sàng")

        remote = provider is not None and not on_device
        self._privacy_title.setText(REMOTE_TITLE if remote else ON_DEVICE_TITLE)
        self._privacy_body.setText(REMOTE_BODY if remote else ON_DEVICE_BODY)

    def _paint_vram(self) -> None:
        snapshot = self.ctx.services.gpu_leases.snapshot()
        capacity = int(snapshot.get("capacity_bytes") or 0)
        reserved = int(snapshot.get("reserved_bytes") or 0)
        leases = list(snapshot.get("leases") or [])
        percent = int(round(reserved / capacity * 100)) if capacity else 0
        self._vram_title.setText(
            "Bộ nhớ hợp nhất cho GPU" if is_unified_memory() else "VRAM đang dùng"
        )
        self._vram_value.setText(f"{format_bytes(reserved)} / {format_bytes(capacity)}")
        self._vram_bar.setValue(max(0, min(100, percent)))
        self._vram_detail.setText(
            f"{len(leases)} mô hình đang dùng" if leases else "Không có mô hình trong GPU"
        )

    # --- models -----------------------------------------------------------

    def _load_models(self) -> None:
        self.ctx.run(self.ctx.services.models.list_models(), on_result=self._paint_models)

    def _paint_models(self, models: Sequence[Any]) -> None:
        entries = [
            ModelEntry(
                name=model.name,
                label=model.name,
                group=str(model.runtime or ""),
                capability=str(model.model_type or ""),
                size_bytes=int(model.size_bytes or 0),
                state=str(model.state or ""),
                detail=format_file_size(int(model.size_bytes or 0)) if model.size_bytes else "",
            )
            for model in models
            if str(model.model_type or "language") == "language"
        ]
        self._model_picker.set_models(entries)
        if not self._model_picker.current():
            default = self.ctx.services.models.default_model("chat")
            if default:
                self._model_picker.set_current(default)
            elif entries:
                self._model_picker.set_current(entries[0].name)

    def _on_model_selected(self, name: str) -> None:
        if not name:
            return
        self.ctx.run(
            repositories.set_model_default(self.ctx.database, "chat", name),
            on_result=lambda _result: self.ctx.modelsChanged.emit(),
            on_error=lambda exc: self.ctx.toast(f"Không đổi được mô hình: {exc}", "error"),
        )

    # --- misc -------------------------------------------------------------

    def _toggle_rail(self, visible: bool) -> None:
        self._rail.setVisible(visible)
        self._rail_toggle.setIcon(icon("panel-right-close" if visible else "panel-right-open"))
        describe(self._rail_toggle, "Ẩn bảng ngữ cảnh" if visible else "Hiện bảng ngữ cảnh")

    def _set_error(self, message: str) -> None:
        self._error.setText(message)
        self._error.setVisible(bool(message))
