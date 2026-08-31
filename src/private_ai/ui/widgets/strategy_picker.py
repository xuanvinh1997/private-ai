"""How the next question searches the library: a labelled chip and a popup that explains.

Three things this replaces a ``QComboBox`` for.

It says what it is. Seven abstract nouns sitting unlabelled beside the model name read as
a property of the model; "Tìm: Tự động" reads as a choice about retrieval.

It explains the options where they are read. Each strategy already had a one-line
description, but a combo box can only carry one per item as a tooltip, so in practice
nobody ever saw them.

It is an override, not a setting. The persistent default lives in Cài đặt; picking here
changes this session only, and the popup says so — the old picker quietly rewrote the
saved default, so one curious click changed how every future conversation retrieved.
"""

from __future__ import annotations

from PySide6.QtCore import QPoint, Qt, Signal
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.ui import icons, theme

__all__ = ["STRATEGY_CHOICES", "StrategyPicker", "strategy_hint", "strategy_label"]

# The strategies the picker offers, in the order it offers them. ``auto`` leads because it
# is the default and the right answer for most questions. ``web`` is deliberately absent:
# the globe toggle beside this control is the one place web search is switched on, and
# offering it here as well gave one word two meanings — a strategy that *replaces* library
# retrieval, and a flag that *adds* web results on top of it.
STRATEGY_CHOICES: tuple[tuple[str, str, str], ...] = (
    (
        RetrievalStrategyName.AUTO.value,
        "Tự động",
        "Tự chọn theo dạng câu hỏi.",
    ),
    (
        RetrievalStrategyName.VECTOR.value,
        "Ngữ nghĩa",
        "Đoạn gần nghĩa nhất với câu hỏi.",
    ),
    (
        RetrievalStrategyName.KEYWORD.value,
        "Từ khóa",
        "Khớp đúng chữ: mã số, tên riêng.",
    ),
    (
        RetrievalStrategyName.HYBRID.value,
        "Kết hợp",
        "Gộp ngữ nghĩa với từ khóa.",
    ),
    (
        RetrievalStrategyName.GRAPH.value,
        "Đồ thị tri thức",
        "Đi theo thực thể và quan hệ.",
    ),
    (
        RetrievalStrategyName.SUMMARY.value,
        "Tóm lược",
        "Đọc trọn tài liệu thay vì từng đoạn.",
    ),
)

_LABELS = {value: label for value, label, _hint in STRATEGY_CHOICES}
_HINTS = {value: hint for value, _label, hint in STRATEGY_CHOICES}

DEFAULT_STRATEGY = STRATEGY_CHOICES[0][0]

POPUP_WIDTH = 360
POPUP_TITLE = "Cách tìm tài liệu"
POPUP_FOOTER = "Chỉ đổi cho phiên này. Mặc định đặt trong Cài đặt."


def strategy_label(value: str) -> str:
    """The caption for a stored value, including one this picker no longer offers."""
    return _LABELS.get(value, value or DEFAULT_STRATEGY)


def strategy_hint(value: str) -> str:
    return _HINTS.get(value, "")


class _ChoiceRow(QPushButton):
    """One option, with the line that says when to reach for it."""

    def __init__(
        self,
        value: str,
        selected: bool,
        is_default: bool,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.value = value
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        # Same shape as the model picker's rows, so the two popups in one toolbar agree.
        self.setProperty("class", "nav-item")
        self.setCheckable(True)
        self.setChecked(selected)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(
            theme.SPACE["md"], theme.SPACE["xs"], theme.SPACE["md"], theme.SPACE["xs"]
        )
        layout.setSpacing(theme.SPACE["sm"])

        copy = QVBoxLayout()
        copy.setContentsMargins(0, 0, 0, 0)
        copy.setSpacing(theme.SPACE["3xs"])
        title = QLabel(strategy_label(value), self)
        title.setProperty("class", "body-strong" if selected else "body")
        copy.addWidget(title)
        hint = QLabel(strategy_hint(value), self)
        hint.setProperty("class", "muted")
        # One line, never wrapped: a ``nav-item`` row is pinned to 40px, so a second line
        # is not shown short — it is shown cut in half.
        copy.addWidget(hint)
        layout.addLayout(copy, 1)

        if is_default:
            badge = QLabel("Mặc định", self)
            badge.setProperty("class", "pill")
            layout.addWidget(badge, 0, Qt.AlignmentFlag.AlignTop)
        if selected:
            check = QLabel(self)
            check.setPixmap(icons.pixmap("check", 16, theme.token("accent")))
            layout.addWidget(check, 0, Qt.AlignmentFlag.AlignTop)

        self.setAccessibleName(f"{strategy_label(value)}, {strategy_hint(value)}")


class _Popup(QFrame):
    chosen = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent, Qt.WindowType.Popup)
        self.setProperty("class", "card")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self.setMinimumWidth(POPUP_WIDTH)

        outer = QVBoxLayout(self)
        outer.setContentsMargins(*(theme.SPACE["sm"],) * 4)
        outer.setSpacing(theme.SPACE["xs"])
        heading = QLabel(POPUP_TITLE, self)
        heading.setProperty("class", "section-label")
        outer.addWidget(heading)

        self._list = QVBoxLayout()
        self._list.setContentsMargins(0, 0, 0, 0)
        self._list.setSpacing(theme.SPACE["3xs"])
        outer.addLayout(self._list)

        divider = QFrame(self)
        divider.setProperty("class", "hline")
        divider.setFrameShape(QFrame.Shape.HLine)
        outer.addWidget(divider)

        footer = QLabel(POPUP_FOOTER, self)
        footer.setProperty("class", "faint")
        footer.setWordWrap(True)
        outer.addWidget(footer)

    def populate(self, current: str, fallback: str) -> None:
        while self._list.count():
            item = self._list.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        for value, _label, _hint in STRATEGY_CHOICES:
            row = _ChoiceRow(value, value == current, value == fallback, self)
            row.clicked.connect(lambda _=False, name=value: self._pick(name))
            self._list.addWidget(row)

    def _pick(self, value: str) -> None:
        self.hide()
        self.chosen.emit(value)


class StrategyPicker(QWidget):
    """The composer's retrieval override.

    ``set_default`` carries the saved preference in; until the user picks something, that
    is what the picker reports, so changing the default in Cài đặt is reflected here
    without this widget ever writing a preference back.
    """

    selectionChanged = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._default = DEFAULT_STRATEGY
        self._override = ""

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        self._trigger = QPushButton(self)
        self._trigger.setProperty("class", "chip")
        self._trigger.setIcon(icons.icon("filter", size=16))
        self._trigger.setCursor(Qt.CursorShape.PointingHandCursor)
        self._trigger.clicked.connect(self._open)
        layout.addWidget(self._trigger)

        self._popup: _Popup | None = None
        self._refresh_trigger()

    # ------------------------------------------------------------------- API

    def set_default(self, value: str) -> None:
        """The saved preference. Ignored for display while an override is in force."""
        self._default = value if value in _LABELS else DEFAULT_STRATEGY
        self._refresh_trigger()

    def set_current(self, value: str) -> None:
        """Force the shown value, as restoring a conversation's last choice would."""
        self._override = value if value in _LABELS else ""
        self._refresh_trigger()

    def current(self) -> str:
        return self._override or self._default

    # -------------------------------------------------------------- internals

    def _refresh_trigger(self) -> None:
        label = strategy_label(self.current())
        self._trigger.setText(f" Tìm: {label}  ▾")
        self._trigger.setToolTip(f"Cách tìm tài liệu: {label}\n{strategy_hint(self.current())}")
        self._trigger.setAccessibleName(f"Cách tìm tài liệu: {label}")

    def _open(self) -> None:
        if self._popup is None:
            self._popup = _Popup(self)
            self._popup.chosen.connect(self._choose)
        self._popup.populate(self.current(), self._default)
        # Width first, then height: the hint lines wrap, so a height computed before the
        # width is settled is a height for the wrong number of lines.
        self._popup.setFixedWidth(max(POPUP_WIDTH, self._trigger.width()))
        self._popup.adjustSize()
        # Opens upward: the picker lives in the composer toolbar at the bottom of the pane.
        above = self.mapToGlobal(QPoint(0, -self._popup.sizeHint().height() - theme.SPACE["sm"]))
        self._popup.move(above)
        self._popup.show()

    def _choose(self, value: str) -> None:
        if value == self.current():
            return
        self._override = value
        self._refresh_trigger()
        self.selectionChanged.emit(value)
