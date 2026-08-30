"""The ingestion progress row: filename, stage, percentage, thin bar.

Three lines of information in one row height, because the context rail shows three of
these at once and the library table shows one per working document. The bar switches to
the danger colour instead of disappearing on failure — a row that vanishes reads as
"finished".
"""

from __future__ import annotations

from PySide6.QtCore import Qt
from PySide6.QtWidgets import QHBoxLayout, QLabel, QProgressBar, QVBoxLayout, QWidget

from private_ai.ui import theme
from private_ai.ui.format import elide, format_percent, stage_label

__all__ = ["IngestionProgress"]


class IngestionProgress(QWidget):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 5, 0, 5)
        layout.setSpacing(5)

        head = QHBoxLayout()
        head.setContentsMargins(0, 0, 0, 0)
        head.setSpacing(8)
        self._title = QLabel(self)
        self._title.setStyleSheet(f"color: {theme.token('ink')}; font-weight: 620;")
        self._percent = QLabel(self)
        self._percent.setProperty("class", "faint")
        self._percent.setAlignment(Qt.AlignmentFlag.AlignRight)
        head.addWidget(self._title, 1)
        head.addWidget(self._percent)
        layout.addLayout(head)

        self._bar = QProgressBar(self)
        self._bar.setRange(0, 100)
        self._bar.setTextVisible(False)
        self._bar.setFixedHeight(6)
        layout.addWidget(self._bar)

        self._detail = QLabel(self)
        self._detail.setProperty("class", "faint")
        self._detail.setWordWrap(False)
        layout.addWidget(self._detail)

    def set_progress(
        self,
        *,
        title: str,
        stage: str = "",
        progress: float = 0.0,
        detail: str = "",
        failed: bool = False,
    ) -> None:
        self._title.setText(elide(title, 42))
        self._title.setToolTip(title)
        fraction = max(0.0, min(1.0, float(progress or 0.0)))
        # An unknown-length stage still has to look alive, so 0% before "completed" runs
        # the bar in busy mode rather than showing an empty trough.
        if fraction <= 0.0 and stage not in ("", "completed", "failed") and not failed:
            self._bar.setRange(0, 0)
            self._percent.setText("")
        else:
            self._bar.setRange(0, 100)
            self._bar.setValue(round(fraction * 100))
            self._percent.setText(format_percent(fraction))
        self._bar.setProperty("class", "danger" if failed else "")
        theme.restyle(self._bar)
        text = detail or stage_label(stage)
        self._detail.setText(elide(text, 60))
        self._detail.setToolTip(text)
        self._detail.setStyleSheet(f"color: {theme.token('danger')};" if failed else "")
        self.setVisible(True)

    def clear(self) -> None:
        self._title.clear()
        self._detail.clear()
        self._percent.clear()
        self._bar.setRange(0, 100)
        self._bar.setValue(0)
        self.setVisible(False)
