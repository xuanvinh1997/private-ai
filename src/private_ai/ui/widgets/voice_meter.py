"""The bars beside the microphone button, and the word that says what they mean.

Dictation has three waits inside it — loading the recogniser, listening, transcribing —
and a button that only swaps its glyph makes all three look identical to a button that did
nothing. So the meter is two things at once: bars that move only when audio is actually
arriving, which is the proof the microphone is live, and a caption naming the phase, which
is the part a still screenshot can be read from.

Painted rather than assembled from styled QWidgets: fourteen bars repainting at 20 Hz are
one ``paintEvent``, and a QSS ``border-radius`` on a 3px-wide widget renders square on
several platform styles.
"""

from __future__ import annotations

from collections import deque

from PySide6.QtCore import QRectF, Qt, QTimer
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import QHBoxLayout, QLabel, QSizePolicy, QWidget

from private_ai.ui import theme

__all__ = ["VoiceMeter"]

STATE_IDLE = "idle"
STATE_PREPARING = "preparing"
STATE_RECORDING = "recording"
STATE_TRANSCRIBING = "transcribing"

# One word per phase, in the order they happen. Rule: the caption says what the app is
# doing, never what the user should do.
CAPTIONS: dict[str, str] = {
    STATE_PREPARING: "Đang mở micro…",
    STATE_RECORDING: "Đang nghe…",
    STATE_TRANSCRIBING: "Đang nhận dạng…",
}

BARS = 14
BAR_WIDTH = 3
BAR_GAP = 2
# 20 Hz: fast enough that the bars read as sound rather than as a progress bar, slow
# enough that an idle laptop stays idle. The timer only runs while the meter is visible.
TICK_MS = 50
# How far a bar falls per tick when the input goes quiet. Without decay the meter freezes
# at the last loud value and looks stuck.
DECAY = 0.12
FLOOR = 0.08


class _Bars(QWidget):
    """A scrolling history of input levels, or a travelling pulse when there is none."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._levels: deque[float] = deque([0.0] * BARS, maxlen=BARS)
        self._level = 0.0
        self._state = STATE_IDLE
        self._phase = 0
        self.setFixedSize(
            BARS * BAR_WIDTH + (BARS - 1) * BAR_GAP,
            theme.CONTROL_HEIGHT,
        )
        self.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)

    def set_state(self, state: str) -> None:
        self._state = state
        if state == STATE_IDLE:
            self._levels = deque([0.0] * BARS, maxlen=BARS)
            self._level = 0.0
        self._phase = 0
        self.update()

    def set_level(self, level: float) -> None:
        self._level = max(0.0, min(1.0, level))

    def tick(self) -> None:
        self._phase = (self._phase + 1) % (BARS * 2)
        if self._state == STATE_RECORDING:
            self._levels.append(self._level)
            # Decay rather than reset: the device delivers a block every few milliseconds
            # and a tick that lands between two of them must not read as silence.
            self._level = max(0.0, self._level - DECAY)
        self.update()

    def paintEvent(self, event) -> None:  # noqa: N802 - Qt override
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setPen(Qt.PenStyle.NoPen)
        # Accent only while audio is genuinely arriving; the two waits are muted, so the
        # colour alone separates "listening" from "working on it".
        live = self._state == STATE_RECORDING
        color = QColor(theme.token("accent" if live else "muted"))
        height = float(self.height())
        span = max(1.0, height - 2 * theme.SPACE["2xs"])
        levels = list(self._levels) if live else self._pulse()
        for index, level in enumerate(levels):
            bar = max(FLOOR, level)
            tint = QColor(color)
            tint.setAlpha(90 + int(165 * bar))
            painter.setBrush(tint)
            length = span * bar
            left = index * (BAR_WIDTH + BAR_GAP)
            painter.drawRoundedRect(
                QRectF(left, (height - length) / 2, BAR_WIDTH, length),
                BAR_WIDTH / 2,
                BAR_WIDTH / 2,
            )
        painter.end()

    def _pulse(self) -> list[float]:
        """A single crest sliding across, for the phases where there is no input yet."""
        head = self._phase if self._phase < BARS else BARS * 2 - self._phase
        return [max(0.0, 1.0 - abs(index - head) / 3.0) * 0.7 for index in range(BARS)]


class VoiceMeter(QWidget):
    """Meter plus caption. Hidden entirely while dictation is idle."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(theme.SPACE["xs"])
        self._bars = _Bars(self)
        self._caption = QLabel(self)
        self._caption.setProperty("class", "muted")
        layout.addWidget(self._bars)
        layout.addWidget(self._caption)

        self._timer = QTimer(self)
        self._timer.setInterval(TICK_MS)
        self._timer.timeout.connect(self._bars.tick)
        self._state = STATE_IDLE
        self.setVisible(False)

    def state(self) -> str:
        return self._state

    def set_state(self, state: str) -> None:
        self._state = state
        self._bars.set_state(state)
        caption = CAPTIONS.get(state, "")
        self._caption.setText(caption)
        # Named for screen readers as one thing: the bars carry no text of their own.
        self.setAccessibleName(caption or "Ghi âm")
        self.setToolTip(caption)
        self.setVisible(bool(caption))
        if caption:
            self._timer.start()
        else:
            self._timer.stop()

    def push_level(self, level: float) -> None:
        self._bars.set_level(level)
