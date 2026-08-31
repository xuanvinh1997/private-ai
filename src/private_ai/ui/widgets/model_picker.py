"""The composer's model chooser: a grouped popup list with a manage-models escape hatch.

Ported from ``apps/web/src/components/ModelPicker.tsx``. A plain ``QComboBox`` cannot show
the second line each row needs (runtime, size, quantisation, whether it is resident in
VRAM), and that second line is the whole reason the picker exists — the user is choosing
between models by what they cost, not by name.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from PySide6.QtCore import QPoint, Qt, Signal
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QVBoxLayout,
    QWidget,
)

from private_ai.ui import icons, theme
from private_ai.ui.format import format_bytes, short_model_name
from private_ai.ui.widgets.status_pip import StatusPip

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

__all__ = ["ModelEntry", "ModelPicker"]

STATE_LABELS = {
    "loaded": "đang trong bộ nhớ",
    "installed": "đã cài đặt",
    "available": "đã cài đặt",
    "unloaded": "chưa nạp",
    "downloading": "đang tải",
    "failed": "lỗi",
    "error": "lỗi",
}

UNSELECTABLE = frozenset({"failed", "error", "downloading"})


@dataclass(frozen=True)
class ModelEntry:
    name: str
    label: str = ""
    group: str = ""
    capability: str = ""
    size_bytes: int = 0
    state: str = ""
    detail: str = ""

    def display(self) -> str:
        return self.label or short_model_name(self.name)

    def meta(self) -> str:
        parts = [part for part in (self.group, self.detail) if part]
        if self.size_bytes:
            parts.append(format_bytes(self.size_bytes))
        if self.capability and self.capability not in ("chat", ""):
            parts.append(
                {"vision": "Đọc ảnh", "embedding": "Embedding"}.get(
                    self.capability, self.capability
                )
            )
        if self.state:
            parts.append(STATE_LABELS.get(self.state, self.state))
        return " · ".join(parts)


class _ModelRow(QPushButton):
    def __init__(self, entry: ModelEntry, selected: bool, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.entry = entry
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setEnabled(entry.state not in UNSELECTABLE)
        # Same shape as the rows in the sidebar: the checked state is what paints the
        # selection, which keeps the whole thing on the stylesheet's palette.
        self.setProperty("class", "nav-item")
        self.setCheckable(True)
        self.setChecked(selected)

        layout = QHBoxLayout(self)
        # A nav row is pinned to 40px, so the two lines of copy own the full height.
        layout.setContentsMargins(theme.SPACE["md"], 0, theme.SPACE["md"], 0)
        layout.setSpacing(theme.SPACE["sm"])

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        title = QLabel(entry.display(), self)
        # Rebuilt per selection, so emphasis is a class swap and the palette stays in the
        # stylesheet; a colour baked in here would survive a theme switch.
        title.setProperty("class", "body-strong" if selected else "body")
        meta = QLabel(entry.meta(), self)
        # The second line is why the picker exists (runtime, size, whether it is resident),
        # so it is muted rather than faint.
        meta.setProperty("class", "muted")
        copy.addWidget(title)
        copy.addWidget(meta)
        layout.addLayout(copy, 1)

        if entry.state == "loaded":
            pip = StatusPip("loaded", self)
            pip.setToolTip("Đang nằm trong bộ nhớ")
            layout.addWidget(pip)
        if selected:
            check = QLabel(self)
            check.setPixmap(icons.pixmap("check", 16, theme.token("accent")))
            layout.addWidget(check)

        self.setAccessibleName(f"{entry.display()}, {entry.meta()}")


class _Popup(QFrame):
    chosen = Signal(str)
    manage = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent, Qt.WindowType.Popup)
        self.setProperty("class", "card")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.setMinimumWidth(320)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(*(theme.SPACE["sm"],) * 4)
        outer.setSpacing(theme.SPACE["xs"])
        heading = QLabel("Chọn mô hình", self)
        heading.setProperty("class", "section-label")
        outer.addWidget(heading)

        self._scroll = QScrollArea(self)
        self._scroll.setWidgetResizable(True)
        self._scroll.setFrameShape(QFrame.Shape.NoFrame)
        self._scroll.setMaximumHeight(360)
        self._body = QWidget(self._scroll)
        self._list = QVBoxLayout(self._body)
        self._list.setContentsMargins(0, 0, 0, 0)
        self._list.setSpacing(theme.SPACE["3xs"])
        self._scroll.setWidget(self._body)
        outer.addWidget(self._scroll)

        divider = QFrame(self)
        divider.setProperty("class", "hline")
        divider.setFrameShape(QFrame.Shape.HLine)
        outer.addWidget(divider)

        footer = QPushButton("  Quản lý mô hình", self)
        footer.setProperty("class", "ghost")
        footer.setIcon(icons.icon("settings-2", size=16))
        footer.clicked.connect(lambda: (self.hide(), self.manage.emit()))
        outer.addWidget(footer)

    def populate(self, entries: Sequence[ModelEntry], current: str, placeholder: str) -> None:
        while self._list.count():
            item = self._list.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        if not entries:
            empty = QLabel(placeholder, self._body)
            empty.setWordWrap(True)
            empty.setProperty("class", "empty")
            self._list.addWidget(empty)
            return
        # Grouped by provider/family, with ungrouped entries first so a single-provider
        # install never grows a pointless heading.
        seen: list[str] = []
        for entry in entries:
            if entry.group and entry.group not in seen:
                seen.append(entry.group)
        multiple = len(seen) > 1
        last_group = None
        for entry in entries:
            if multiple and entry.group and entry.group != last_group:
                last_group = entry.group
                label = QLabel(entry.group, self._body)
                label.setProperty("class", "section-label")
                self._list.addWidget(label)
            row = _ModelRow(entry, entry.name == current, self._body)
            row.clicked.connect(lambda _=False, name=entry.name: self._pick(name))
            self._list.addWidget(row)

    def _pick(self, name: str) -> None:
        self.hide()
        self.chosen.emit(name)


class ModelPicker(QWidget):
    selectionChanged = Signal(str)
    manageRequested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._entries: list[ModelEntry] = []
        self._current = ""
        self._placeholder = "Chưa có mô hình trò chuyện nào."

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._trigger = QPushButton(self)
        self._trigger.setProperty("class", "chip")
        self._trigger.setIcon(icons.icon("boxes", size=16))
        self._trigger.setCursor(Qt.CursorShape.PointingHandCursor)
        self._trigger.clicked.connect(self._open)
        layout.addWidget(self._trigger)

        self._popup: _Popup | None = None
        self._refresh_trigger()

    # ------------------------------------------------------------------- API
    def set_models(self, entries: Sequence[ModelEntry]) -> None:
        self._entries = list(entries)
        if self._current and all(entry.name != self._current for entry in self._entries):
            self._current = ""
        self._refresh_trigger()

    def set_current(self, name: str) -> None:
        if name == self._current:
            return
        self._current = name or ""
        self._refresh_trigger()

    def current(self) -> str:
        return self._current

    def set_placeholder(self, text: str) -> None:
        self._placeholder = text or self._placeholder
        self._refresh_trigger()

    # -------------------------------------------------------------- internals
    def _label(self) -> str:
        if self._current:
            return short_model_name(self._current)
        return "Chưa có mô hình" if self._entries else "Đang tải mô hình…"

    def _refresh_trigger(self) -> None:
        self._trigger.setText(f" {self._label()}  ▾")
        self._trigger.setToolTip(f"Mô hình: {self._label()}")
        self._trigger.setAccessibleName(self._trigger.toolTip())

    def _open(self) -> None:
        if self._popup is None:
            self._popup = _Popup(self)
            self._popup.chosen.connect(self._choose)
            self._popup.manage.connect(self.manageRequested)
        self._popup.populate(self._entries, self._current, self._placeholder)
        self._popup.adjustSize()
        width = max(self._popup.width(), self._trigger.width())
        self._popup.setFixedWidth(width)
        # Opens upward: the picker lives in the composer toolbar at the bottom of the pane.
        below = self.mapToGlobal(QPoint(0, -self._popup.sizeHint().height() - theme.SPACE["sm"]))
        self._popup.move(below)
        self._popup.show()

    def _choose(self, name: str) -> None:
        if name == self._current:
            return
        self._current = name
        self._refresh_trigger()
        self.selectionChanged.emit(name)
