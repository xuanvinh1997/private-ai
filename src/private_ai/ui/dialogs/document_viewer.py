"""Read one document's extracted text.

The point of this dialog is to answer "what did the machine actually read?" — a citation
chip in chat and a row in the library both land here. When the answer is "nothing", the
one useful action is offered: read it again with OCR.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QStackedWidget,
    QTextBrowser,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui import markdown as md
from private_ai.ui import theme
from private_ai.ui.dialogs import _shell
from private_ai.ui.format import badge_class, format_file_size, status_label
from private_ai.ui.icons import icon

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.ui.context import AppContext

__all__ = ["DocumentViewer"]

_LOADING = 0
_CONTENT = 1
_EMPTY = 2

EMPTY_TITLE = "Chưa có nội dung nào được trích xuất"
EMPTY_HINT = "Tài liệu có thể là bản scan chưa qua OCR."
REREAD_LABEL = "Đọc lại có OCR"
REREAD_BUSY = "Đang đọc lại…"


class DocumentViewer(QDialog):
    """Modeless on purpose: the user keeps reading while the answer streams behind it."""

    def __init__(
        self,
        ctx: AppContext,
        document_id: str,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._document_id = document_id
        self._working = False
        # Kept because a re-render is the only way a new palette reaches text already laid
        # out: the stylesheet is the document's *default*, applied when the HTML is set.
        self._text = ""

        self.setWindowTitle("Tài liệu")
        self.setModal(False)
        self.resize(760, 620)
        self._build()
        ctx.themeChanged.connect(self._on_theme_changed)
        self.reload()

    # --- layout -----------------------------------------------------------

    def _build(self) -> None:
        layout = _shell.dialog_layout(self)

        # The status rides beside the title as a badge; the line under it is left to the
        # facts that have no state of their own.
        self._status_badge = QLabel()
        self._status_badge.hide()
        self._title, self._subtitle = _shell.title_block(
            layout,
            "Đang mở tài liệu",
            "Đang đọc nội dung đã trích xuất…",
            trailing=self._status_badge,
        )

        self._error = QLabel()
        self._error.setProperty("class", "danger")
        self._error.setWordWrap(True)
        self._error.hide()
        layout.addWidget(self._error)

        self._stack = QStackedWidget()
        self._stack.addWidget(self._loading_page())
        self._stack.addWidget(self._content_page())
        self._stack.addWidget(self._empty_page())
        layout.addWidget(self._stack, 1)

        actions = _shell.action_row(layout)
        actions.addStretch(1)
        close = QPushButton("Đóng")
        close.setProperty("class", "primary")
        close.setDefault(True)
        close.clicked.connect(self.accept)
        actions.addWidget(close)

    @staticmethod
    def _loading_page() -> QWidget:
        page = QWidget()
        box = QVBoxLayout(page)
        box.setContentsMargins(0, 0, 0, 0)
        label = QLabel("Đang đọc nội dung…")
        label.setProperty("class", "muted")
        label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        box.addWidget(label)
        return page

    def _content_page(self) -> QWidget:
        self._body = QTextBrowser()
        self._body.setOpenExternalLinks(True)
        # Qt's rich-text CSS has no max-width, so the reading measure is capped here.
        self._body.setMaximumWidth(md.READING_MEASURE_PX)
        self._body.document().setDefaultStyleSheet(md.document_css(theme.tokens()))
        return self._body

    def _empty_page(self) -> QWidget:
        page = QWidget()
        box = QVBoxLayout(page)
        box.setContentsMargins(0, 0, 0, 0)
        box.setSpacing(theme.SPACE["sm"])
        box.addStretch(1)
        self._empty_title = QLabel(EMPTY_TITLE)
        self._empty_title.setProperty("class", "heading")
        self._empty_title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty_detail = QLabel(EMPTY_HINT)
        self._empty_detail.setProperty("class", "muted")
        self._empty_detail.setWordWrap(True)
        self._empty_detail.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._reread = QPushButton(icon("refresh-cw"), REREAD_LABEL)
        self._reread.clicked.connect(self._on_reread)
        row = QHBoxLayout()
        row.setSpacing(theme.TOOLBAR_SPACING)
        row.addStretch(1)
        row.addWidget(self._reread)
        row.addStretch(1)
        box.addWidget(self._empty_title)
        box.addWidget(self._empty_detail)
        box.addLayout(row)
        box.addStretch(1)
        return page

    def _restyle_body(self) -> None:
        self._body.document().setDefaultStyleSheet(md.document_css(theme.tokens()))
        self._body.setHtml(md.markdown_to_html(self._text))

    def _on_theme_changed(self, _name: str) -> None:
        """This dialog is modeless, so it outlives the switch that repaints everything
        else; the already-rendered document keeps the old palette until it is re-set."""
        if self._text.strip():
            self._restyle_body()

    # --- data -------------------------------------------------------------

    def reload(self) -> None:
        self._stack.setCurrentIndex(_LOADING)
        self._ctx.run(
            repositories.get_document(self._ctx.database, self._document_id),
            on_result=self._apply,
            on_error=self._on_load_error,
        )

    def _apply(self, document: dict[str, Any]) -> None:
        filename = str(document.get("filename") or "Tài liệu")
        self.setWindowTitle(filename)
        self._title.setText(filename)

        state = str(document.get("status") or "")
        self._status_badge.setText(status_label(state))
        self._status_badge.setProperty("class", badge_class(state))
        theme.restyle(self._status_badge)
        self._status_badge.setVisible(bool(state))

        size = format_file_size(int(document.get("byte_size") or 0))
        ocr = "không dùng OCR" if document.get("use_ocr") == 0 else "OCR theo mặc định"
        self._subtitle.setText(f"{size} · {ocr}")

        self._text = str(document.get("extracted_text") or "")
        if self._text.strip():
            self._restyle_body()
            self._stack.setCurrentIndex(_CONTENT)
            return
        self._empty_detail.setText(str(document.get("error") or EMPTY_HINT))
        self._stack.setCurrentIndex(_EMPTY)

    def _on_load_error(self, exc: BaseException) -> None:
        self._show_error(f"Không đọc được tài liệu: {exc}")
        self._stack.setCurrentIndex(_EMPTY)

    def _show_error(self, message: str) -> None:
        self._error.setText(message)
        self._error.setVisible(bool(message))

    # --- actions ----------------------------------------------------------

    def _on_reread(self) -> None:
        if self._working:
            return
        self._working = True
        self._reread.setEnabled(False)
        self._reread.setText(REREAD_BUSY)
        self._show_error("")
        self._ctx.run(
            self._reprocess(),
            on_result=lambda _result: self._finish_reread(""),
            on_error=lambda exc: self._finish_reread(f"Không thể đọc lại tài liệu: {exc}"),
        )

    async def _reprocess(self) -> None:
        """Flip OCR on permanently, then run it here rather than waiting on the worker.

        ``process`` takes the cross-process claim, so if the worker already owns this
        document the call returns immediately and the reload below still shows its result.
        """
        database = self._ctx.database
        await repositories.queue_document(database, self._document_id, use_ocr=True)
        await self._ctx.services.ingestion.process(self._document_id)

    def _finish_reread(self, message: str) -> None:
        self._working = False
        self._reread.setEnabled(True)
        self._reread.setText(REREAD_LABEL)
        self._show_error(message)
        if not message:
            self._ctx.documentsChanged.emit()
        self.reload()
