"""Stage files, choose OCR per file, then ingest them one at a time.

Two things carried over from the web version. Files are *staged* rather than uploaded on
drop, because the OCR choice belongs to each file and has to be made before anything is
read. And the queue is strictly sequential: ingestion holds a GPU lease for embedding, so
two files at once is slower than one after another, not faster.

One thing is different. There is no HTTP upload any more, so instead of polling a status
endpoint the dialog passes a ``ProgressSink`` straight into ``IngestionPipeline.process``
and watches the stages arrive. Polling survives only for the case that still needs it: the
worker process holding the cross-process claim, where ``process`` returns immediately
having done nothing.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
from time import monotonic
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QMimeData, Qt, Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.core import repositories
from private_ai.ui.format import format_file_size
from private_ai.ui.icons import icon

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Iterable, Sequence

    from PySide6.QtGui import QDragEnterEvent, QDropEvent

    from private_ai.ui.context import AppContext

__all__ = ["ACCEPTED_EXTENSIONS", "MAX_FILE_BYTES", "UploadDialog", "accepted_paths"]

ACCEPTED_EXTENSIONS = (
    ".pdf",
    ".docx",
    ".pptx",
    ".xlsx",
    ".txt",
    ".md",
    ".markdown",
    ".csv",
    ".json",
    ".yaml",
    ".yml",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
    ".gif",
    ".bmp",
    ".tif",
    ".tiff",
)
MAX_FILE_BYTES = 100 * 1024 * 1024
POLL_INTERVAL_SECONDS = 1.0
POLL_TIMEOUT_SECONDS = 5 * 60

_FILE_FILTER = "Tài liệu ({});;Tất cả tệp (*)".format(
    " ".join(f"*{extension}" for extension in ACCEPTED_EXTENSIONS)
)

# Progress a status alone implies, when no job row has been written yet.
_STATUS_PROGRESS = {"queued": 45, "processing": 78, "ready": 100, "needs_ocr": 100, "failed": 100}

# Stages after the text exists: the file is no longer being read, it is being indexed.
_INDEXING_STAGES = frozenset(
    {"chunking", "embedding", "graph", "multimodal", "finalizing", "indexing"}
)

_IN_FLIGHT = frozenset({"uploading", "queued", "processing", "indexing"})
_ACTIONABLE = frozenset({"pending", "failed", "needs_ocr"})

_STATUS_TEXT = {
    "pending": "Sẵn sàng tải lên",
    "uploading": "Đang thêm vào thư viện",
    "queued": "Đã nhận tệp · đang chờ OCR",
    "processing": "Đang OCR và chuẩn bị lập chỉ mục",
    "indexing": "OCR xong · đang tạo embedding và graph memory",
    "ready": "Đã xử lý xong",
    "needs_ocr": "OCR chưa đọc được nội dung",
    "failed": "Xử lý thất bại",
    "invalid": "Tệp không hợp lệ",
}


def _validate(path: Path, size: int) -> str:
    if not size:
        return "Tệp trống"
    if size > MAX_FILE_BYTES:
        return "Vượt quá giới hạn 100 MB"
    if path.suffix.lower() not in ACCEPTED_EXTENSIONS:
        return "Định dạng chưa được hỗ trợ"
    return ""


def accepted_paths(mime: QMimeData) -> list[Path]:
    """Local files out of a drag payload, in drop order. Directories are ignored."""
    if not mime.hasUrls():
        return []
    paths: list[Path] = []
    for url in mime.urls():
        if not url.isLocalFile():
            continue
        candidate = Path(url.toLocalFile())
        if candidate.is_file():
            paths.append(candidate)
    return paths


@dataclass
class _Staged:
    path: Path
    size: int
    mtime: float
    use_ocr: bool
    status: str = "pending"
    progress: int = 0
    document_id: str = ""
    error: str = ""
    detail: str = ""
    vectors_per_second: float = 0.0

    @property
    def key(self) -> tuple[str, int, float]:
        # Same identity the web app used: two picks of the same file are one file.
        return (self.path.name, self.size, self.mtime)

    def label(self) -> str:
        rate = ""
        if self.vectors_per_second:
            value = self.vectors_per_second
            shown = f"{value:.1f}" if value < 10 else str(round(value))
            rate = f" · {shown} vector/s"
        if self.status in _IN_FLIGHT and self.detail:
            return f"{self.detail} · {self.progress}%{rate}"
        if self.status == "invalid":
            return self.error or _STATUS_TEXT["invalid"]
        return _STATUS_TEXT.get(self.status, self.status)


class _StagedRow(QFrame):
    removeRequested = Signal()
    ocrToggled = Signal(bool)

    def __init__(self, item: _Staged, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._item = item
        self._locked = False
        self.setProperty("class", "card")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(10, 8, 8, 8)
        layout.setSpacing(10)

        self._mark = QLabel()
        self._mark.setFixedWidth(20)
        layout.addWidget(self._mark, 0, Qt.AlignmentFlag.AlignTop)

        identity = QVBoxLayout()
        identity.setSpacing(3)
        self._name = QLabel(item.path.name)
        self._name.setToolTip(str(item.path))
        self._name.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self._status = QLabel()
        self._status.setProperty("class", "muted")
        self._status.setWordWrap(True)
        self._bar = QProgressBar()
        self._bar.setRange(0, 100)
        self._bar.setTextVisible(False)
        self._bar.setFixedHeight(4)
        identity.addWidget(self._name)
        identity.addWidget(self._status)
        identity.addWidget(self._bar)
        layout.addLayout(identity, 1)

        self._ocr = QCheckBox("OCR")
        self._ocr.setToolTip("Đọc chữ trong ảnh hoặc tài liệu scan")
        self._ocr.setChecked(item.use_ocr)
        self._ocr.toggled.connect(self.ocrToggled)
        layout.addWidget(self._ocr, 0, Qt.AlignmentFlag.AlignTop)

        self._remove = QToolButton()
        self._remove.setIcon(icon("x", size=16))
        self._remove.setToolTip(f"Bỏ {item.path.name} khỏi danh sách")
        self._remove.clicked.connect(self.removeRequested)
        layout.addWidget(self._remove, 0, Qt.AlignmentFlag.AlignTop)

        self.refresh(item)

    def refresh(self, item: _Staged) -> None:
        self._item = item
        size = format_file_size(item.size)
        text = f"{size} · {item.label()}"
        if item.error and item.status != "invalid":
            text = f"{text} — {item.error}"
        self._status.setText(text)
        broken = item.status in {"failed", "needs_ocr", "invalid"}
        name = "loader" if item.status in _IN_FLIGHT else "check" if item.status == "ready" else ""
        self._mark.setPixmap(
            icon("alert-triangle" if broken else name or "file-text", size=16).pixmap(16, 16)
        )
        self._bar.setVisible(item.status not in {"pending", "invalid"})
        self._bar.setValue(max(0, min(100, item.progress)))
        self._apply_lock()

    def set_locked(self, locked: bool) -> None:
        self._locked = locked
        self._apply_lock()

    def _apply_lock(self) -> None:
        item = self._item
        settled = item.status in _IN_FLIGHT or item.status in {"ready", "invalid"}
        self._ocr.setEnabled(not self._locked and not settled)
        self._remove.setEnabled(not self._locked and item.status not in _IN_FLIGHT)


class UploadDialog(QDialog):
    """Add documents to one workspace. Emits ``completed`` with a per-run tally."""

    completed = Signal(dict)

    def __init__(
        self,
        ctx: AppContext,
        *,
        workspace_id: str = "",
        workspace_name: str = "",
        files: Sequence[Path] | None = None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._workspace_id = workspace_id or ctx.workspace_id
        self._workspace_name = workspace_name
        self._staged: list[_Staged] = []
        self._rows: list[_StagedRow] = []
        self._busy = False
        self._detached = False

        self.setWindowTitle("Thêm tài liệu")
        self.setAcceptDrops(True)
        self.resize(660, 620)
        self._build()
        self.add_files(files or [])

    # --- layout -----------------------------------------------------------

    def _build(self) -> None:
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 18, 20, 16)
        layout.setSpacing(10)

        title = QLabel("Thêm tài liệu")
        title.setProperty("class", "title")
        layout.addWidget(title)

        self._subtitle = QLabel()
        self._subtitle.setProperty("class", "subtitle")
        self._subtitle.setWordWrap(True)
        layout.addWidget(self._subtitle)

        self._drop = QPushButton(icon("upload"), "Chọn tệp từ máy")
        self._drop.setMinimumHeight(84)
        self._drop.setToolTip(
            "Hoặc kéo thả · PDF, Office, ảnh và văn bản · tối đa 100 MB mỗi tệp",
        )
        self._drop.clicked.connect(self._pick_files)
        layout.addWidget(self._drop)

        self._heading = QLabel()
        self._heading.setProperty("class", "section-label")
        layout.addWidget(self._heading)

        self._list_host = QWidget()
        self._list_layout = QVBoxLayout(self._list_host)
        self._list_layout.setContentsMargins(0, 0, 0, 0)
        self._list_layout.setSpacing(6)
        self._list_layout.addStretch(1)

        scroller = QScrollArea()
        scroller.setWidgetResizable(True)
        scroller.setFrameShape(QFrame.Shape.NoFrame)
        scroller.setWidget(self._list_host)
        layout.addWidget(scroller, 1)

        self._notice = QLabel()
        self._notice.setProperty("class", "muted")
        self._notice.setWordWrap(True)
        self._notice.hide()
        layout.addWidget(self._notice)

        self._error = QLabel()
        self._error.setProperty("class", "danger")
        self._error.setWordWrap(True)
        self._error.hide()
        layout.addWidget(self._error)

        self._overall = QProgressBar()
        self._overall.setRange(0, 1)
        self._overall.setFormat("Đã xử lý %v/%m tệp")
        self._overall.hide()
        layout.addWidget(self._overall)

        actions = QHBoxLayout()
        actions.addStretch(1)
        self._cancel = QPushButton("Hủy")
        self._cancel.clicked.connect(self.reject)
        self._confirm = QPushButton("Tải lên")
        self._confirm.setProperty("class", "primary")
        self._confirm.setDefault(True)
        self._confirm.clicked.connect(self._on_confirm)
        actions.addWidget(self._cancel)
        actions.addWidget(self._confirm)
        layout.addLayout(actions)

        self._refresh_chrome()

    # --- staging ----------------------------------------------------------

    def add_files(self, paths: Iterable[Path]) -> None:
        known = {item.key for item in self._staged}
        skipped = 0
        for path in paths:
            try:
                stat = path.stat()
            except OSError as exc:
                self._staged.append(
                    _Staged(path, 0, 0.0, False, status="invalid", error=str(exc)),
                )
                continue
            item = _Staged(
                path=path,
                size=stat.st_size,
                mtime=stat.st_mtime,
                use_ocr=self._ctx.preferences.ocr_enabled,
            )
            if item.key in known:
                skipped += 1
                continue
            known.add(item.key)
            problem = _validate(path, item.size)
            if problem:
                item.status = "invalid"
                item.error = problem
            self._staged.append(item)
        self._rebuild_rows()
        self._set_notice(f"{skipped} tệp trùng đã được bỏ qua." if skipped else "")

    def _pick_files(self) -> None:
        paths, _selected = QFileDialog.getOpenFileNames(
            self,
            "Chọn tài liệu",
            "",
            _FILE_FILTER,
        )
        self.add_files(Path(path) for path in paths)

    def _rebuild_rows(self) -> None:
        while self._rows:
            row = self._rows.pop()
            self._list_layout.removeWidget(row)
            row.deleteLater()
        for position, item in enumerate(self._staged):
            row = _StagedRow(item)
            row.removeRequested.connect(lambda index=position: self._remove(index))
            row.ocrToggled.connect(lambda value, index=position: self._set_ocr(index, value))
            row.set_locked(self._busy)
            self._list_layout.insertWidget(self._list_layout.count() - 1, row)
            self._rows.append(row)
        self._refresh_chrome()

    def _remove(self, index: int) -> None:
        if self._busy or not 0 <= index < len(self._staged):
            return
        del self._staged[index]
        self._rebuild_rows()

    def _set_ocr(self, index: int, value: bool) -> None:
        if 0 <= index < len(self._staged):
            self._staged[index].use_ocr = value

    def _refresh_row(self, index: int) -> None:
        if 0 <= index < len(self._rows):
            self._rows[index].refresh(self._staged[index])

    def _refresh_chrome(self) -> None:
        if self._workspace_id:
            self._subtitle.setText(
                f"Tải vào {self._workspace_name or 'không gian đang chọn'}. "
                "Có thể bật OCR riêng cho từng tệp."
            )
        else:
            self._subtitle.setText(
                "Tạo một không gian làm việc trước, sau đó quay lại để thêm tài liệu."
            )
        invalid = sum(1 for item in self._staged if item.status == "invalid")
        parts = [f"{len(self._staged)} tệp đã chọn"] if self._staged else []
        if invalid:
            parts.append(f"{invalid} cần bỏ hoặc thay thế")
        self._heading.setText(" · ".join(parts))
        self._heading.setVisible(bool(parts))
        self._drop.setEnabled(not self._busy and bool(self._workspace_id))
        self._confirm.setEnabled(
            not self._busy and bool(self._workspace_id) and bool(self._actionable())
        )
        self._confirm.setText(self._action_label())
        self._cancel.setText("Đóng · tiếp tục nền" if self._busy else "Hủy")

    def _actionable(self) -> list[int]:
        return [index for index, item in enumerate(self._staged) if item.status in _ACTIONABLE]

    def _action_label(self) -> str:
        if self._busy:
            return f"Đang xử lý {self._overall.value()}/{max(1, self._overall.maximum())}"
        pending = self._actionable()
        failed = sum(1 for item in self._staged if item.status in {"failed", "needs_ocr"})
        if failed:
            return f"Thử lại {len(pending)} tệp"
        return f"Tải lên {len(pending)} tệp" if pending else "Tải lên"

    def _set_notice(self, message: str) -> None:
        self._notice.setText(message)
        self._notice.setVisible(bool(message))

    def _set_error(self, message: str) -> None:
        self._error.setText(message)
        self._error.setVisible(bool(message))

    # --- drag and drop ----------------------------------------------------

    def dragEnterEvent(self, event: QDragEnterEvent) -> None:  # noqa: N802
        if not self._busy and self._workspace_id and accepted_paths(event.mimeData()):
            event.acceptProposedAction()

    def dropEvent(self, event: QDropEvent) -> None:  # noqa: N802
        paths = accepted_paths(event.mimeData())
        if not paths:
            return
        event.acceptProposedAction()
        self.add_files(paths)

    # --- ingestion --------------------------------------------------------

    def _on_confirm(self) -> None:
        if self._busy:
            return
        if not self._workspace_id:
            self._set_error("Hãy tạo một không gian làm việc trước khi thêm tài liệu.")
            return
        if not self._actionable():
            return
        self._ctx.run(
            self._run(),
            on_result=lambda _result: None,
            on_error=self._on_run_error,
        )

    async def _run(self) -> None:
        queue = self._actionable()
        self._busy = True
        self._set_error("")
        self._set_notice("")
        self._overall.setRange(0, len(queue))
        self._overall.setValue(0)
        self._overall.show()
        for row in self._rows:
            row.set_locked(True)
        self._refresh_chrome()
        try:
            uploaded = await self._ingest(queue)
            await self._await_worker(queue)
            self._report(queue, uploaded)
        finally:
            self._busy = False
            self._overall.hide()
            for index, row in enumerate(self._rows):
                row.set_locked(False)
                if index < len(self._staged):
                    row.refresh(self._staged[index])
            self._refresh_chrome()
            if self._detached and not self.isVisible():
                self.deleteLater()

    async def _ingest(self, queue: Sequence[int]) -> int:
        ingestion = self._ctx.services.ingestion
        database = self._ctx.database
        uploaded = 0
        for position, index in enumerate(queue):
            item = self._staged[index]
            try:
                if item.document_id:
                    self._patch(index, status="queued", progress=45, error="")
                    await repositories.queue_document(
                        database,
                        item.document_id,
                        use_ocr=item.use_ocr,
                    )
                else:
                    self._patch(index, status="uploading", progress=1, error="")
                    item.document_id = await ingestion.add_file(
                        item.path,
                        self._workspace_id,
                        use_ocr=item.use_ocr,
                    )
                    uploaded += 1
                    self._patch(index, status="queued", progress=45)
                await ingestion.process(item.document_id, on_progress=self._sink(index))
                self._apply_document(
                    index,
                    await repositories.get_document(
                        database,
                        item.document_id,
                    ),
                )
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                self._patch(
                    index,
                    status="failed",
                    progress=100,
                    error=str(exc) or "Không thể tải lên",
                )
            self._overall.setValue(position + 1)
            self._refresh_chrome()
        return uploaded

    def _sink(self, index: int) -> Any:
        """A ProgressSink bound to one row. Called from the pipeline on this same loop."""

        def report(stage: str, progress: float, detail: str = "") -> None:
            status = "processing"
            if stage in _INDEXING_STAGES:
                status = "indexing"
            elif stage == "completed":
                status = "ready"
            elif stage in {"failed", "needs_ocr"}:
                status = stage
            elif stage == "queued":
                status = "queued"
            self._patch(
                index,
                status=status,
                progress=int(round(max(0.0, min(1.0, progress)) * 100)),
                detail=detail,
            )

        return report

    async def _await_worker(self, queue: Sequence[int]) -> None:
        """Poll only what we could not process ourselves — a worker holds its claim."""
        database = self._ctx.database
        deadline = monotonic() + POLL_TIMEOUT_SECONDS
        errors = 0
        while monotonic() < deadline:
            active = [index for index in queue if self._staged[index].status in _IN_FLIGHT]
            self._overall.setValue(len(queue) - len(active))
            if not active:
                return
            await asyncio.sleep(POLL_INTERVAL_SECONDS)
            try:
                for index in active:
                    document_id = self._staged[index].document_id
                    if not document_id:
                        continue
                    self._apply_document(
                        index,
                        await repositories.get_document(
                            database,
                            document_id,
                        ),
                    )
                errors = 0
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                errors += 1
                if errors < 3:
                    continue
                self._set_error(f"Không đọc được trạng thái xử lý: {exc}")
                return

    def _patch(self, index: int, **changes: Any) -> None:
        if not 0 <= index < len(self._staged):
            return
        item = self._staged[index]
        for key, value in changes.items():
            setattr(item, key, value)
        self._refresh_row(index)

    def _apply_document(self, index: int, document: dict[str, Any]) -> None:
        ingestion = document.get("ingestion") or {}
        step = str(ingestion.get("step") or "")
        running = str(ingestion.get("status") or "") == "processing"
        status = str(document.get("status") or "failed")
        if running:
            status = "indexing" if step in _INDEXING_STAGES else "processing"
        if ingestion:
            progress = int(round(float(ingestion.get("progress") or 0.0) * 100))
        elif document.get("status") == "ready" and not document.get("indexed_at"):
            progress = 42
        else:
            progress = _STATUS_PROGRESS.get(str(document.get("status") or ""), 0)
        self._patch(
            index,
            document_id=str(document.get("id") or ""),
            status=status,
            progress=progress,
            error=str(ingestion.get("error") or document.get("error") or ""),
            detail=str(ingestion.get("detail") or ""),
            vectors_per_second=float(ingestion.get("vectors_per_second") or 0.0),
        )

    def _report(self, queue: Sequence[int], uploaded: int) -> None:
        finished = [self._staged[index] for index in queue]
        ready = sum(1 for item in finished if item.status == "ready")
        failed = sum(1 for item in finished if item.status in {"failed", "needs_ocr"})
        pending = sum(1 for item in finished if item.status in _IN_FLIGHT)
        self._ctx.documentsChanged.emit()
        self.completed.emit(
            {"uploaded": uploaded, "ready": ready, "failed": failed, "pending": pending}
        )
        if failed:
            self._set_error(
                f"{failed} tệp xử lý chưa thành công. Xem lỗi ngay dưới tên tệp rồi thử lại."
            )
            return
        if pending:
            self._set_notice(
                "Tệp vẫn đang được xử lý nền. Trạng thái sẽ tiếp tục cập nhật trong Thư viện."
            )
            return
        if all(item.status == "ready" for item in self._staged):
            self.accept()
            return
        if any(item.status == "invalid" for item in self._staged):
            self._set_notice(
                "Các tệp hợp lệ đã xử lý xong. Bỏ hoặc thay thế tệp không hợp lệ còn lại."
            )

    def _on_run_error(self, exc: BaseException) -> None:
        self._busy = False
        self._overall.hide()
        self._set_error(str(exc) or "Không thể tải tài liệu lên")
        self._refresh_chrome()

    # --- closing ----------------------------------------------------------

    def reject(self) -> None:
        # A dismissed run keeps going, as it did on the web: the pipeline is mid-embed and
        # cancelling would throw the work away. Hiding keeps this object alive for it.
        if self._busy:
            self._detached = True
            self.hide()
            return
        super().reject()
