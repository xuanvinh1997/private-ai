"""The 8px dot that says whether something is alive.

Painted rather than styled: a QSS ``border-radius`` on a tiny QLabel renders as a square
on several platform styles, and this dot appears next to almost every status line in the
app.
"""

from __future__ import annotations

from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import QHBoxLayout, QLabel, QSizePolicy, QWidget

from private_ai.ui import theme

__all__ = ["StatusPip", "StatusPipLabel", "state_color", "state_text"]

# Every state the health endpoint and the model registry can report, mapped onto three
# colours; anything unknown reads as "faint", never as "fine".
_TONE: dict[str, str] = {
    "online": "success",
    "ok": "success",
    "ready": "success",
    "loaded": "success",
    "healthy": "success",
    "warn": "warn",
    "degraded": "warn",
    "busy": "warn",
    "downloading": "warn",
    "not_configured": "warn",
    "offline": "danger",
    "error": "danger",
    "failed": "danger",
}

_LABELS: dict[str, str] = {
    "online": "Đang hoạt động",
    "ok": "Đang hoạt động",
    "ready": "Sẵn sàng",
    "loaded": "Trong bộ nhớ",
    "warn": "Cần chú ý",
    "degraded": "Hoạt động hạn chế",
    "busy": "Đang bận",
    "downloading": "Đang tải",
    "not_configured": "Chưa cấu hình",
    "offline": "Ngoại tuyến",
    "error": "Lỗi",
    "failed": "Lỗi",
    "unknown": "Chưa rõ",
}


def state_color(state: str) -> str:
    return theme.token(_TONE.get((state or "").strip().lower(), "faint"))


def state_text(state: str) -> str:
    key = (state or "").strip().lower()
    return _LABELS.get(key, key or "Chưa rõ")


class StatusPip(QWidget):
    def __init__(self, state: str = "unknown", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._state = state
        self.setFixedSize(QSize(theme.SPACE["sm"], theme.SPACE["sm"]))
        self.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
        self.setToolTip(state_text(state))

    def state(self) -> str:
        return self._state

    def set_state(self, state: str) -> None:
        if state == self._state:
            return
        self._state = state
        self.setToolTip(state_text(state))
        self.update()

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        color = QColor(state_color(self._state))
        painter.setPen(Qt.PenStyle.NoPen)
        # The halo is what makes an 8px dot readable against both the sidebar and a card.
        halo = QColor(color)
        halo.setAlpha(60)
        painter.setBrush(halo)
        painter.drawEllipse(self.rect())
        painter.setBrush(color)
        painter.drawEllipse(self.rect().adjusted(2, 2, -2, -2))
        painter.end()


class StatusPipLabel(QWidget):
    """A pip and its caption, the pairing used in the topbar and the context rail."""

    def __init__(
        self,
        text: str = "",
        state: str = "unknown",
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(theme.SPACE["xs"])
        self.pip = StatusPip(state, self)
        self.label = QLabel(text, self)
        self.label.setProperty("class", "muted")
        layout.addWidget(self.pip)
        layout.addWidget(self.label, 1)

    def set_state(self, state: str) -> None:
        self.pip.set_state(state)

    def set_text(self, text: str) -> None:
        self.label.setText(text)
