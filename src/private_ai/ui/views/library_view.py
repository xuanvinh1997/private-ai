"""The document library: search, filter, live ingestion progress, paging."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui.format import (
    format_file_size,
    format_relative_time,
    stage_label,
    status_label,
)
from private_ai.ui.icons import icon
from private_ai.ui.widgets.confirm_button import ConfirmButton

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.ui.context import AppContext

PAGE_SIZE = 20
SEARCH_DEBOUNCE_MS = 250
POLL_INTERVAL_MS = 1500

STATUS_FILTERS: tuple[tuple[str, str], ...] = (
    ("", "Tất cả"),
    ("ready", "Sẵn sàng"),
    ("processing", "Đang xử lý"),
    ("needs_ocr", "Cần OCR"),
    ("failed", "Lỗi"),
)

_BUSY_STATUSES = frozenset({"queued", "processing"})


def _file_kind(filename: str) -> str:
    _, separator, extension = filename.rpartition(".")
    if not separator or not extension:
        return "TXT"
    return extension[:4].upper()


def _is_busy(document: dict[str, Any]) -> bool:
    if str(document.get("status") or "") in _BUSY_STATUSES:
        return True
    ingestion = document.get("ingestion") or {}
    return str(ingestion.get("status") or "") == "processing"


def _progress(document: dict[str, Any]) -> float:
    ingestion = document.get("ingestion") or {}
    return float(ingestion.get("progress") or 0.08)


class _DocumentRow(QFrame):
    def __init__(self, view: LibraryView, document: dict[str, Any], parent=None) -> None:
        super().__init__(parent)
        self._view = view
        self._document = document
        document_id = str(document["id"])
        filename = str(document["filename"])
        busy = _is_busy(document)

        self.setProperty("class", "card")
        layout = QHBoxLayout(self)
        layout.setSpacing(12)

        badge = QLabel(_file_kind(filename))
        badge.setProperty("class", "pill")
        badge.setFixedWidth(52)
        badge.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(badge, 0, Qt.AlignmentFlag.AlignTop)

        copy = QVBoxLayout()
        copy.setSpacing(3)
        open_button = QPushButton(filename)
        open_button.setFlat(True)
        open_button.setProperty("class", "ghost")
        open_button.setCursor(Qt.CursorShape.PointingHandCursor)
        open_button.setEnabled(not busy)
        open_button.setToolTip(f"{filename} vẫn đang xử lý" if busy else f"Xem nội dung {filename}")
        open_button.clicked.connect(lambda: view.open_document(document_id))
        copy.addWidget(open_button, 0, Qt.AlignmentFlag.AlignLeft)

        meta = [
            format_file_size(int(document.get("byte_size") or 0)),
            format_relative_time(document.get("created_at")),
        ]
        error = str(document.get("error") or "").strip()
        if error:
            meta.append(error)
        subtitle = QLabel(" · ".join(meta))
        subtitle.setWordWrap(True)
        subtitle.setProperty("class", "danger" if error else "muted")
        copy.addWidget(subtitle)

        ingestion = document.get("ingestion") or {}
        if busy and ingestion:
            fraction = _progress(document)
            detail = QLabel(
                f"{ingestion.get('detail') or stage_label(str(ingestion.get('step') or ''))} · "
                f"{round(fraction * 100)}%"
            )
            detail.setProperty("class", "faint")
            copy.addWidget(detail)
            bar = QProgressBar()
            bar.setRange(0, 100)
            bar.setValue(int(fraction * 100))
            bar.setTextVisible(False)
            bar.setFixedHeight(6)
            copy.addWidget(bar)
            vectors = int(ingestion.get("embedded_vectors") or 0)
            if vectors > 0:
                rate = float(ingestion.get("vectors_per_second") or 0)
                rate_text = f"{rate:.1f}" if rate < 10 else f"{round(rate)}"
                elapsed = round(float(ingestion.get("elapsed_seconds") or 0))
                pace = QLabel(f"{vectors} vector · {rate_text} vector/s · {elapsed} giây")
                pace.setProperty("class", "faint")
                copy.addWidget(pace)
        layout.addLayout(copy, 1)

        status = QLabel(
            f"{stage_label(str(ingestion.get('step') or 'extracting'))} "
            f"{round(_progress(document) * 100)}%"
            if busy
            else status_label(str(document.get("status") or ""))
        )
        status.setProperty("class", "chip")
        layout.addWidget(status, 0, Qt.AlignmentFlag.AlignTop)

        actions = QHBoxLayout()
        actions.setSpacing(6)
        state = str(document.get("status") or "")
        if state == "needs_ocr":
            ocr = QPushButton("Đọc lại có OCR")
            ocr.clicked.connect(lambda: view.requeue(document_id, True))
            actions.addWidget(ocr)
        if state in {"failed", "needs_ocr"}:
            retry = QToolButton()
            retry.setIcon(icon("refresh-cw"))
            retry.setToolTip(f"Xử lý lại {filename}")
            retry.clicked.connect(lambda: view.requeue(document_id, None))
            actions.addWidget(retry)
        remove = ConfirmButton("Xóa", "Xóa hẳn", icon_name="trash-2")
        remove.confirmed.connect(lambda: view.remove(document_id, filename))
        actions.addWidget(remove)
        layout.addLayout(actions, 0)


class LibraryView(QWidget):
    def __init__(self, ctx: AppContext, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._page = 0
        self._search = ""
        self._status = ""
        self._total = 0
        self._items: list[dict[str, Any]] = []
        self._summary: dict[str, Any] = {}
        self._loading = False
        self._active = False
        self._viewers: list[QWidget] = []

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 20, 24, 20)
        root.setSpacing(12)

        heading = QHBoxLayout()
        titles = QVBoxLayout()
        eyebrow = QLabel("Thư viện riêng")
        eyebrow.setProperty("class", "section-label")
        titles.addWidget(eyebrow)
        self._title = QLabel("Tài liệu")
        self._title.setProperty("class", "title")
        titles.addWidget(self._title)
        self._stats = QLabel("")
        self._stats.setWordWrap(True)
        self._stats.setProperty("class", "muted")
        titles.addWidget(self._stats)
        heading.addLayout(titles, 1)
        upload = QPushButton("Thêm tài liệu")
        upload.setIcon(icon("upload"))
        upload.setProperty("class", "primary")
        upload.clicked.connect(self._on_upload)
        heading.addWidget(upload, 0, Qt.AlignmentFlag.AlignTop)
        root.addLayout(heading)

        toolbar = QHBoxLayout()
        self._search_box = QLineEdit()
        self._search_box.setClearButtonEnabled(True)
        self._search_box.setPlaceholderText("Tìm theo tên tệp")
        self._search_box.addAction(icon("search"), QLineEdit.ActionPosition.LeadingPosition)
        self._search_box.textChanged.connect(self._queue_search)
        toolbar.addWidget(self._search_box, 1)

        self._chips: dict[str, QPushButton] = {}
        for value, label in STATUS_FILTERS:
            chip = QPushButton(label)
            chip.setCheckable(True)
            chip.setProperty("class", "chip")
            chip.setChecked(value == "")
            chip.clicked.connect(lambda _=False, v=value: self._set_status(v))
            self._chips[value] = chip
            toolbar.addWidget(chip)
        root.addLayout(toolbar)

        self._empty = QLabel("")
        self._empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self._empty.setWordWrap(True)
        self._empty.setProperty("class", "empty")
        root.addWidget(self._empty)

        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._canvas = QWidget()
        self._rows = QVBoxLayout(self._canvas)
        self._rows.setSpacing(8)
        self._rows.setContentsMargins(0, 0, 0, 0)
        self._rows.addStretch(1)
        self._scroll.setWidget(self._canvas)
        root.addWidget(self._scroll, 1)

        pager = QHBoxLayout()
        self._range = QLabel("")
        self._range.setProperty("class", "faint")
        pager.addWidget(self._range, 1)
        self._previous = QToolButton()
        self._previous.setIcon(icon("chevron-left"))
        self._previous.setToolTip("Trang trước")
        self._previous.clicked.connect(lambda: self._go(self._page - 1))
        pager.addWidget(self._previous)
        self._page_label = QLabel("")
        pager.addWidget(self._page_label)
        self._next = QToolButton()
        self._next.setIcon(icon("chevron-right"))
        self._next.setToolTip("Trang sau")
        self._next.clicked.connect(lambda: self._go(self._page + 1))
        pager.addWidget(self._next)
        root.addLayout(pager)

        self._debounce = QTimer(self)
        self._debounce.setSingleShot(True)
        self._debounce.setInterval(SEARCH_DEBOUNCE_MS)
        self._debounce.timeout.connect(self._commit_search)

        self._poll = QTimer(self)
        self._poll.setInterval(POLL_INTERVAL_MS)
        self._poll.timeout.connect(self._on_poll)

        ctx.workspaceChanged.connect(self._on_workspace)
        ctx.documentsChanged.connect(self.refresh)
        self._load_title()

    # --- lifecycle --------------------------------------------------------

    def on_activated(self) -> None:
        self._active = True
        self.refresh()

    def on_deactivated(self) -> None:
        self._active = False
        self._poll.stop()

    def _on_workspace(self, _workspace_id: str) -> None:
        self._page = 0
        self._load_title()
        self.refresh()

    def _load_title(self) -> None:
        """The heading is the workspace's own name, as it was in the web app."""
        workspace_id = self._ctx.workspace_id
        if not workspace_id:
            self._title.setText("Tài liệu")
            return
        self._ctx.run(
            repositories.get_workspace(self._ctx.database, workspace_id),
            on_result=lambda record: self._title.setText(record.name),
            on_error=lambda _: self._title.setText("Tài liệu"),
        )

    def _on_poll(self) -> None:
        # A hidden window costs nothing to leave un-refreshed, and polling behind it only
        # burns CPU on a laptop the user has walked away from.
        window = self._ctx.window
        if window is not None and not window.isVisible():
            return
        if not self._active:
            self._poll.stop()
            return
        self.refresh()

    # --- filters ----------------------------------------------------------

    def _queue_search(self, _text: str) -> None:
        self._debounce.start()

    def _commit_search(self) -> None:
        term = self._search_box.text().strip()
        if term == self._search:
            return
        self._search = term
        self._page = 0
        self.refresh()

    def _set_status(self, value: str) -> None:
        self._status = value
        for key, chip in self._chips.items():
            chip.setChecked(key == value)
        self._page = 0
        self.refresh()

    def _go(self, page: int) -> None:
        pages = max(1, -(-self._total // PAGE_SIZE))
        self._page = max(0, min(page, pages - 1))
        self.refresh()

    # --- data -------------------------------------------------------------

    def refresh(self) -> None:
        workspace_id = self._ctx.workspace_id
        if not workspace_id:
            self._items = []
            self._render()
            return
        if self._loading:
            return
        self._loading = True
        self._ctx.run(
            repositories.list_documents(
                self._ctx.database,
                workspace_id,
                q=self._search,
                status=self._status,
                limit=PAGE_SIZE,
                offset=self._page * PAGE_SIZE,
            ),
            on_result=self._loaded,
            on_error=self._failed,
        )

    def _loaded(self, payload: dict[str, Any]) -> None:
        self._loading = False
        self._items = list(payload.get("items") or [])
        self._total = int(payload.get("total") or 0)
        self._summary = dict(payload.get("summary") or {})
        self._render()
        if any(_is_busy(item) for item in self._items):
            if self._active and not self._poll.isActive():
                self._poll.start()
        else:
            self._poll.stop()

    def _failed(self, exc: BaseException) -> None:
        self._loading = False
        self._ctx.toast(str(exc) or "Không đọc được thư viện", "error")

    # --- rendering --------------------------------------------------------

    def _clear_rows(self) -> None:
        while self._rows.count() > 1:
            item = self._rows.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

    def _render(self) -> None:
        self._clear_rows()
        total = int(self._summary.get("total") or 0)
        parts = []
        if total:
            parts.append(f"{total} tài liệu")
            parts.append(format_file_size(int(self._summary.get("byte_size") or 0)))
            if int(self._summary.get("pending") or 0):
                parts.append(f"{int(self._summary['pending'])} đang xử lý")
            if int(self._summary.get("indexing") or 0):
                parts.append(f"{int(self._summary['indexing'])} đang lập chỉ mục")
            if int(self._summary.get("failed") or 0):
                parts.append(f"{int(self._summary['failed'])} cần xem lại")
            self._stats.setText(" · ".join(parts))
        else:
            self._stats.setText("Chưa có tài liệu nào trong không gian này.")

        filtering = bool(self._search or self._status)
        has_workspace = bool(self._ctx.workspace_id)
        self._search_box.setVisible(has_workspace and total > 0)
        for chip in self._chips.values():
            chip.setVisible(has_workspace and total > 0)

        if not has_workspace:
            self._show_empty("Hãy mở một không gian làm việc để xem tài liệu của nó.")
            return
        if not self._items:
            self._show_empty(
                "Không có tài liệu nào khớp. Thử từ khóa khác hoặc bỏ bộ lọc trạng thái."
                if filtering
                else "Chưa có tài liệu nào.\nThêm tệp từ đây, hoặc kéo thả vào màn trò chuyện."
            )
            return

        self._empty.hide()
        self._scroll.show()
        for document in self._items:
            self._rows.insertWidget(self._rows.count() - 1, _DocumentRow(self, document))

        start = self._page * PAGE_SIZE + 1
        end = self._page * PAGE_SIZE + len(self._items)
        suffix = " (đã lọc)" if filtering else ""
        self._range.setText(f"{start}–{end} trong {self._total}{suffix}")
        pages = max(1, -(-self._total // PAGE_SIZE))
        self._page_label.setText(f"Trang {self._page + 1}/{pages}")
        self._page_label.setVisible(pages > 1)
        self._previous.setVisible(pages > 1)
        self._next.setVisible(pages > 1)
        self._previous.setEnabled(self._page > 0)
        self._next.setEnabled(self._page < pages - 1)

    def _show_empty(self, message: str) -> None:
        self._empty.setText(message)
        self._empty.show()
        self._scroll.hide()
        self._range.setText("")
        self._page_label.hide()
        self._previous.hide()
        self._next.hide()

    # --- actions ----------------------------------------------------------

    def open_document(self, document_id: str) -> None:
        from private_ai.ui.dialogs.document_viewer import DocumentViewer

        # Modeless by design, so it is held open by a reference rather than by `exec`.
        viewer = DocumentViewer(self._ctx, document_id, self)
        viewer.finished.connect(lambda _: self.refresh())
        self._viewers.append(viewer)
        viewer.destroyed.connect(lambda _=None, ref=viewer: self._forget_viewer(ref))
        viewer.show()

    def _forget_viewer(self, viewer) -> None:
        if viewer in self._viewers:
            self._viewers.remove(viewer)

    def _on_upload(self) -> None:
        if not self._ctx.workspace_id:
            self._ctx.toast("Hãy chọn một không gian làm việc trước", "info")
            return
        from private_ai.ui.dialogs.upload_dialog import UploadDialog

        dialog = UploadDialog(
            self._ctx,
            workspace_id=self._ctx.workspace_id,
            workspace_name=self._title.text(),
            parent=self,
        )
        dialog.completed.connect(lambda _: self.refresh())
        dialog.exec()
        self.refresh()

    def requeue(self, document_id: str, use_ocr: bool | None) -> None:
        self._ctx.run(
            repositories.queue_document(self._ctx.database, document_id, use_ocr=use_ocr),
            on_result=lambda _: self._requeued(),
            on_error=lambda exc: self._ctx.toast(
                str(exc) or "Không thể xử lý lại tài liệu", "error"
            ),
        )

    def _requeued(self) -> None:
        self._ctx.toast("Đã đưa tài liệu vào hàng chờ", "success")
        self._ctx.documentsChanged.emit()
        self.refresh()

    def remove(self, document_id: str, filename: str) -> None:
        self._ctx.run(
            self._ctx.services.ingestion.delete_document(document_id, confirmed=True),
            on_result=lambda _: self._removed(filename),
            on_error=lambda exc: self._ctx.toast(str(exc) or "Không thể xóa tài liệu", "error"),
        )

    def _removed(self, filename: str) -> None:
        self._ctx.toast(f"Đã xóa {filename}", "success")
        self._ctx.documentsChanged.emit()
        self.refresh()
