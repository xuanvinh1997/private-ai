"""The live trail of what the turn is doing, shown where the answer will appear.

Three things separate "slow" from "hung", and the old single motionless label had none of
them: a step that *changes*, a clock that keeps moving even when the step does not, and a
record of how far the turn already got. A retrieval that map-reduces a long document can
run for minutes; during that time the only honest thing the UI can say is which chunk it
is on and how long it has been going.

The clock is the load-bearing part. A step can legitimately sit still for a minute while
the model loads, so the seconds are what tell the user the app is still alive — which is
why the timer ticks off its own ``QTimer`` rather than off the arrival of events.
"""

from __future__ import annotations

from PySide6.QtCore import QTimer
from PySide6.QtWidgets import QHBoxLayout, QLabel, QVBoxLayout, QWidget

from private_ai.ui import icons, theme
from private_ai.ui.widgets.status_pip import StatusPip

__all__ = ["ReasoningTrail"]

TICK_MS = 1000
# How long one step may sit silent before the trail marks itself as still alive. Under
# this, the ticking clock is reassurance enough; past it, the user starts to wonder.
QUIET_SECONDS = 12
# A glyph, not a third clause: the line already carries a step, a clock and a detail, and
# user is scanning for movement. The wording lives in the tooltip instead.
QUIET_NOTE = "Vẫn đang chạy"
_INDICATOR_PX = 14
# Steps older than this are dropped from the top of the trail. Enough to see the path
# taken, not so many that the bubble outgrows the answer it is standing in for.
MAX_DONE_STEPS = 4
# Notices outlive the steps, so they get their own, tighter cap.
MAX_NOTES = 3
# Below this, nobody wondered whether the app had hung, so the summary line is noise
# under the answer rather than reassurance next to it.
MIN_SUMMARY_SECONDS = 3


def _elapsed_text(seconds: int) -> str:
    if seconds < 60:
        return f"{seconds} giây"
    minutes, rest = divmod(seconds, 60)
    return f"{minutes} phút {rest:02d} giây" if rest else f"{minutes} phút"


class _Step(QWidget):
    """One line of the trail: a pip, what happened, and any detail it carried."""

    def __init__(self, label: str, detail: str = "", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        row = QHBoxLayout(self)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(theme.SPACE["sm"])
        self.pip = StatusPip("busy", self)
        row.addWidget(self.pip, 0)
        self._label = QLabel(label, self)
        self._label.setProperty("class", "muted")
        row.addWidget(self._label, 0)
        self._detail = QLabel(detail, self)
        self._detail.setProperty("class", "faint")
        self._detail.setVisible(bool(detail))
        row.addWidget(self._detail, 0)
        self._quiet = False
        self._indicator = QLabel(self)
        self._indicator.setPixmap(
            icons.pixmap("loader", size=_INDICATOR_PX, color=theme.token("faint"))
        )
        self._indicator.setFixedSize(_INDICATOR_PX, _INDICATOR_PX)
        self._indicator.setToolTip(QUIET_NOTE)
        self._indicator.setAccessibleName(QUIET_NOTE)
        self._indicator.setVisible(False)
        row.addWidget(self._indicator, 0)
        row.addStretch(1)

    def set_text(self, label: str, detail: str) -> None:
        self._label.setText(label)
        self._detail.setText(detail)
        self._detail.setVisible(bool(detail))

    def set_quiet(self, quiet: bool) -> None:
        self._quiet = quiet
        self._indicator.setVisible(quiet)

    def is_quiet(self) -> bool:
        return self._quiet

    def set_state(self, state: str) -> None:
        self.pip.set_state(state)


class ReasoningTrail(QWidget):
    """Steps already taken, plus the one running now with a clock on it."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._seconds = 0
        self._quiet = 0
        self._running = False
        self._failed = False
        self._done: list[_Step] = []
        self._notes: list[_Step] = []
        # The current step is held as data and rendered from it. Reading the label back
        # out of the widget to re-render it is how the clock ends up inside the detail.
        self._label = ""
        self._detail = ""

        column = QVBoxLayout(self)
        column.setContentsMargins(0, 0, 0, 0)
        column.setSpacing(theme.SPACE["2xs"])
        # Notices sit in their own box because they outlive the steps: a degraded
        # sub-service is still the answer to "why is there no citation" long after the
        # path taken has been collapsed away.
        self._notes_box = QVBoxLayout()
        self._notes_box.setContentsMargins(0, 0, 0, 0)
        self._notes_box.setSpacing(theme.SPACE["2xs"])
        column.addLayout(self._notes_box)

        self._done_box = QVBoxLayout()
        self._done_box.setContentsMargins(0, 0, 0, 0)
        self._done_box.setSpacing(theme.SPACE["2xs"])
        column.addLayout(self._done_box)

        self._current = _Step("")
        column.addWidget(self._current)

        # Its own clock, so the seconds keep moving through a step that does not.
        self._timer = QTimer(self)
        self._timer.setInterval(TICK_MS)
        self._timer.timeout.connect(self._tick)

    # --- lifecycle --------------------------------------------------------

    def start(self, label: str) -> None:
        self.reset()
        self._current.setVisible(True)
        self._running = True
        self._failed = False
        self._label = label
        self._render()
        self._timer.start()

    def reset(self) -> None:
        self._timer.stop()
        self._running = False
        self._failed = False
        self._seconds = 0
        self._quiet = 0
        self._clear_done()
        while self._notes_box.count():
            item = self._notes_box.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        self._notes.clear()
        self._label = ""
        self._detail = ""
        self._current.set_text("", "")
        self._current.set_state("busy")

    def collapse(self) -> None:
        """Drop the finished steps, keep the clock running.

        Called when the answer starts arriving: the path taken has served its purpose and
        the trail must not push the text it was standing in for down the bubble.
        """
        self._clear_done()

    def step(self, label: str, detail: str = "") -> None:
        """Move to a new step, filing the previous one as done."""
        if self._failed:
            return
        if not self._running:
            self.start(label)
        if self._label and self._label != label:
            self._retire(self._label, self._detail)
        self._quiet = 0
        self._label = label
        self._detail = detail
        self._render()

    def note(self, message: str) -> None:
        """A degraded sub-service: kept beside the answer, in warning colour."""
        step = _Step(message)
        step.set_state("warn")
        self._notes_box.addWidget(step)
        self._notes.append(step)
        while len(self._notes) > MAX_NOTES:
            stale = self._notes.pop(0)
            self._notes_box.removeWidget(stale)
            stale.deleteLater()

    def finish(self, label: str = "") -> None:
        """Stop the clock and collapse the trail to one line of what it cost.

        A failed turn keeps its message: the turn still ends, and ``_finish_turn`` runs
        after the error, so without this guard every failure would read as "Xong".
        """
        self._timer.stop()
        self._running = False
        if self._failed:
            return
        self._clear_done()
        self._label = label or "Xong"
        self._detail = ""
        self._current.set_text(self._label, _elapsed_text(self._seconds))
        self._current.set_state("ready")
        self._current.setVisible(self._seconds >= MIN_SUMMARY_SECONDS)

    def fail(self, message: str) -> None:
        self._timer.stop()
        self._running = False
        self._failed = True
        self._clear_done()
        self._label = message
        self._detail = ""
        self._current.set_text(message, _elapsed_text(self._seconds))
        self._current.set_state("failed")

    def elapsed(self) -> int:
        return self._seconds

    def has_content(self) -> bool:
        """Whether anything is left worth showing, once the turn has ended."""
        return self._current.isVisible() or bool(self._notes)

    # --- internals --------------------------------------------------------

    def _tick(self) -> None:
        self._seconds += 1
        self._quiet += 1
        self._render()

    def _render(self) -> None:
        parts = [_elapsed_text(self._seconds)]
        if self._detail:
            parts.append(self._detail)
        self._current.set_text(self._label, " ".join(parts))
        self._current.set_quiet(self._quiet >= QUIET_SECONDS)
        self._current.set_state("busy")

    def _retire(self, label: str, detail: str) -> None:
        step = _Step(label, detail)
        step.set_state("ready")
        self._done_box.addWidget(step)
        self._done.append(step)
        self._trim()

    def _clear_done(self) -> None:
        while self._done_box.count():
            item = self._done_box.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        self._done.clear()

    def _trim(self) -> None:
        while len(self._done) > MAX_DONE_STEPS:
            step = self._done.pop(0)
            self._done_box.removeWidget(step)
            step.deleteLater()
