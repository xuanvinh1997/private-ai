"""The turn has to keep saying where it is, or a slow turn reads as a crashed one.

Two halves are asserted here: that the graph actually reports its stages onto the stream
(the summary strategy has always produced this detail — the bug was that nobody passed it
a sink), and that the widget showing it keeps a clock running through a step that does
not change, which is the only signal distinguishing "loading a model" from "hung".
"""

from __future__ import annotations

from typing import Any

import pytest

from private_ai.agent import progress


def test_a_stage_outside_a_run_is_a_no_op() -> None:
    """Nodes report unconditionally; a unit test calling one directly must not blow up."""
    progress.emit("retrieve", "12 nguồn", 0.5)
    progress.notice("bộ nhớ không đọc được")


def test_every_stage_has_a_human_label() -> None:
    for stage, label in progress.STAGE_LABELS.items():
        assert label and label != stage, f"{stage} has no caption"
    # An unknown stage must still render as something, never as an empty line.
    assert progress.stage_label("chưa-biết") == "chưa-biết"
    assert progress.stage_label("") == "Đang xử lý"


def test_a_sink_maps_a_subtask_onto_its_share_of_the_turn() -> None:
    """A summary reporting 1.0 has finished the summary, not the turn."""
    seen: list[dict[str, Any]] = []

    def capture(payload: dict[str, Any]) -> None:
        seen.append(payload)

    report = progress.sink(offset=0.2, span=0.5)
    with _writer(capture):
        report("mapping", 0.0, "Tóm tắt tài liệu 1/4")
        report("mapping", 1.0, "Tóm tắt tài liệu 4/4")

    assert [item["fraction"] for item in seen] == [0.2, 0.7]
    assert seen[0]["label"] == "Đọc tài liệu"
    assert seen[0]["detail"] == "Tóm tắt tài liệu 1/4"


def test_a_sink_clamps_a_subtask_that_overshoots() -> None:
    seen: list[dict[str, Any]] = []
    with _writer(seen.append):
        progress.sink()("mapping", 4.2)
    assert seen[0]["fraction"] == 1.0


class _writer:
    """Stand in for LangGraph's stream writer for the duration of a block."""

    def __init__(self, capture) -> None:
        self._capture = capture
        self._patched = None

    def __enter__(self):
        import langgraph.config as config

        self._patched = config.get_stream_writer
        config.get_stream_writer = lambda: self._capture
        return self

    def __exit__(self, *exc: object) -> None:
        import langgraph.config as config

        config.get_stream_writer = self._patched


# --- the widget -----------------------------------------------------------


@pytest.fixture
def trail(qapp):
    from private_ai.ui import theme
    from private_ai.ui.widgets.reasoning_trail import ReasoningTrail

    theme.apply_theme(qapp, "light", "normal")
    widget = ReasoningTrail()
    widget.resize(420, 240)
    widget.show()
    qapp.processEvents()
    yield widget
    widget.close()


def test_the_clock_runs_through_a_step_that_does_not_change(trail, qapp) -> None:
    """The whole point: a model load holds one step for a minute and must still move."""
    trail.start("Đang suy nghĩ")
    trail.step("Nạp mô hình", "qwen3:8b")
    before = _detail(trail)
    for _ in range(3):
        trail._tick()
    qapp.processEvents()
    assert trail.elapsed() == 3
    assert _detail(trail) != before
    assert "3 giây" in _detail(trail)


def test_a_long_silence_raises_the_still_running_indicator(trail) -> None:
    """The mark is a glyph, not a clause: the detail text must stay the step's own."""
    from private_ai.ui.widgets.reasoning_trail import QUIET_NOTE, QUIET_SECONDS

    trail.start("Đang suy nghĩ")
    trail.step("Đọc tài liệu", "Tóm tắt tài liệu 7/31")
    for _ in range(QUIET_SECONDS):
        trail._tick()
    assert trail._current.is_quiet()
    assert QUIET_NOTE not in _detail(trail)
    # A new step resets the patience, because something visibly happened.
    trail.step("Soạn câu trả lời")
    assert not trail._current.is_quiet()


def test_the_path_taken_is_capped(trail) -> None:
    from private_ai.ui.widgets.reasoning_trail import MAX_DONE_STEPS

    trail.start("Đang suy nghĩ")
    for index in range(MAX_DONE_STEPS + 4):
        trail.step(f"Bước {index}")
    assert len(trail._done) == MAX_DONE_STEPS


def test_a_failure_survives_the_turn_ending(trail) -> None:
    """``_finish_turn`` runs after the error handler, so ``finish`` must not overwrite it."""
    trail.start("Đang suy nghĩ")
    trail.fail("Không kết nối được máy chủ")
    trail.finish()
    assert _label(trail) == "Không kết nối được máy chủ"
    # And a late progress event from a stream still draining cannot revive it.
    trail.step("Soạn câu trả lời")
    assert _label(trail) == "Không kết nối được máy chủ"


def test_notices_outlive_the_steps(trail) -> None:
    """A degraded sub-service answers "why is there no citation" long after the fact."""
    trail.start("Đang suy nghĩ")
    trail.step("Tìm trong thư viện")
    trail.note("Không truy hồi được tài liệu: index lỗi")
    trail.collapse()
    assert len(trail._done) == 0
    assert len(trail._notes) == 1
    trail.finish()
    assert len(trail._notes) == 1
    trail.reset()
    assert len(trail._notes) == 0


def test_finishing_reports_what_the_turn_cost(trail) -> None:
    trail.start("Đang suy nghĩ")
    for _ in range(65):
        trail._tick()
    trail.finish()
    assert "1 phút 05 giây" in _detail(trail)


def test_a_fast_turn_leaves_nothing_behind(trail) -> None:
    """ "Xong · 0 giây" under an instant answer is noise, not reassurance."""
    trail.start("Đang suy nghĩ")
    trail.finish()
    assert not trail.has_content()


def test_a_slow_turn_says_what_it_cost(trail) -> None:
    from private_ai.ui.widgets.reasoning_trail import MIN_SUMMARY_SECONDS

    trail.start("Đang suy nghĩ")
    for _ in range(MIN_SUMMARY_SECONDS):
        trail._tick()
    trail.finish()
    assert trail.has_content()
    assert _detail(trail) == f"{MIN_SUMMARY_SECONDS} giây"


def test_a_fast_turn_that_degraded_still_says_so(trail) -> None:
    trail.start("Đang suy nghĩ")
    trail.note("Không đọc được bộ nhớ cá nhân")
    trail.finish()
    assert trail.has_content()


def _label(trail) -> str:
    return trail._current._label.text()


def _detail(trail) -> str:
    return trail._current._detail.text()
