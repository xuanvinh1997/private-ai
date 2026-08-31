"""Pull an Ollama model, with live progress and a cancel that really aborts.

``ModelAdmin.pull`` streams NDJSON with no timeout, so the only thing that stops a
multi-gigabyte download is closing the generator — which is what setting the cancel event
and letting the loop return does.
"""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QDialog,
    QLabel,
    QLineEdit,
    QProgressBar,
    QPushButton,
)

from private_ai.llm.admin import pull_fraction
from private_ai.ui.dialogs import _shell

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.ui.context import AppContext


def _describe(event: dict[str, Any]) -> str:
    status = str(event.get("status") or "").strip() or "Đang tải"
    total = event.get("total")
    completed = event.get("completed")
    if isinstance(total, int | float) and total > 0 and isinstance(completed, int | float):
        return f"{status} · {completed / 1e9:.2f}/{total / 1e9:.2f} GB"
    return status


class AddModelDialog(QDialog):
    completed = Signal(str)  # model name

    def __init__(self, ctx: AppContext, parent=None) -> None:
        super().__init__(parent)
        self._ctx = ctx
        self._cancel = asyncio.Event()
        self._task = None

        self.setModal(True)
        self.setWindowTitle("Tải mô hình Ollama")
        self.setMinimumWidth(480)

        layout = _shell.dialog_layout(self)
        _shell.title_block(
            layout,
            "Tải mô hình Ollama",
            "Nhập tên trong thư viện Ollama, ví dụ qwen3:8b. Bạn có thể theo dõi tiến trình "
            "ngay tại đây.",
        )

        self._name = QLineEdit()
        self._name.setPlaceholderText("qwen3:8b")
        self._name.returnPressed.connect(self._on_pull)
        _shell.field(layout, "Tên mô hình", self._name)

        self._bar = QProgressBar()
        self._bar.setRange(0, 100)
        self._bar.setTextVisible(False)
        self._bar.hide()
        layout.addWidget(self._bar)

        self._status = QLabel("")
        self._status.setWordWrap(True)
        self._status.setProperty("class", "muted")
        self._status.hide()
        layout.addWidget(self._status)

        self._error = QLabel("")
        self._error.setWordWrap(True)
        self._error.setProperty("class", "danger")
        self._error.hide()
        layout.addWidget(self._error)

        row = _shell.action_row(layout)
        row.addStretch(1)
        self._cancel_button = QPushButton("Hủy")
        self._cancel_button.clicked.connect(self._on_cancel)
        row.addWidget(self._cancel_button)
        self._start = QPushButton("Bắt đầu tải")
        self._start.setProperty("class", "primary")
        self._start.setDefault(True)
        self._start.clicked.connect(self._on_pull)
        row.addWidget(self._start)

        self._name.setFocus(Qt.FocusReason.OtherFocusReason)

    # --- lifecycle --------------------------------------------------------

    def reject(self) -> None:
        self._abort()
        super().reject()

    def closeEvent(self, event) -> None:  # noqa: N802
        self._abort()
        super().closeEvent(event)

    def _abort(self) -> None:
        self._cancel.set()
        task = self._task
        self._task = None
        if task is not None and not task.done():
            task.cancel()

    def _on_cancel(self) -> None:
        if self._task is not None and not self._task.done():
            self._abort()
            self._status.setText("Đã hủy tải.")
            self._running(False)
            return
        self.reject()

    # --- pull -------------------------------------------------------------

    def _running(self, running: bool) -> None:
        self._start.setEnabled(not running)
        self._start.setText("Đang tải…" if running else "Bắt đầu tải")
        self._name.setEnabled(not running)
        self._bar.setVisible(running)
        self._cancel_button.setText("Dừng tải" if running else "Hủy")

    def _on_pull(self) -> None:
        name = self._name.text().strip()
        if not name or (self._task is not None and not self._task.done()):
            return
        self._error.hide()
        self._status.setText("Đang kết nối…")
        self._status.show()
        self._bar.setValue(0)
        self._running(True)
        self._cancel = asyncio.Event()
        self._task = self._ctx.run(
            self._pull(name),
            on_result=lambda _: None,
            on_error=self._failed,
        )

    async def _pull(self, name: str) -> None:
        admin = self._ctx.services.models.admin
        try:
            async for event in admin.pull(name, cancel=self._cancel):
                if self._cancel.is_set():
                    return
                self._status.setText(_describe(event))
                fraction = pull_fraction(event)
                if fraction:
                    self._bar.setValue(int(fraction * 100))
                if str(event.get("error") or "").strip():
                    raise RuntimeError(str(event["error"]))
        except asyncio.CancelledError:
            raise
        if self._cancel.is_set():
            return
        self._running(False)
        self._ctx.toast(f"Đã tải xong {name}", "success")
        self.completed.emit(name)
        self.accept()

    def _failed(self, exc: BaseException) -> None:
        self._running(False)
        self._status.hide()
        self._error.setText(str(exc) or "Không thể tải mô hình")
        self._error.show()
