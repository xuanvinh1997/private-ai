"""The two notification surfaces, held to one vocabulary and one set of markers.

Two defects are guarded here because both were invisible in a diff and obvious on screen:

* the bell and the toast kept **separate** tone maps that had drifted into contradicting
  each other — the same "info" event was a green tick in one and a blue circle in the
  other;
* a notice row marked itself unread with ``setStyleSheet("QFrame { border-left: … }")``,
  and since ``QLabel`` *is* a ``QFrame``, every label inside the row got its own green
  bracket and rounded plate.
"""

from __future__ import annotations

import time

import pytest

from private_ai.ui import theme
from private_ai.ui.format import notice_tone

pytestmark = pytest.mark.usefixtures("qapp")

NOTICE = "Không nghe được gì. Kiểm tra micro rồi ghi lại."


def settle(qapp, predicate, timeout: float = 2.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline and not predicate():
        qapp.processEvents()
        time.sleep(0.01)
    qapp.processEvents()


@pytest.fixture
def overlay(qapp):
    from PySide6.QtWidgets import QWidget

    from private_ai.ui.widgets.toast import ToastOverlay

    theme.apply_theme(qapp, "light", "normal")
    host = QWidget()
    host.resize(1280, 860)
    host.show()
    made = ToastOverlay(host)
    yield made
    host.close()


def build_notice(tone: str = "info", unread: bool = True, **extra):
    from private_ai.ui.widgets.notifications import Notice, _NoticeRow

    notice = Notice(id="1", title="Nhà cung cấp không phản hồi", tone=tone, **extra)
    row = _NoticeRow(notice, unread)
    row.resize(360, 72)
    return row


# --- one vocabulary --------------------------------------------------------


@pytest.mark.parametrize(
    ("alias", "canonical"),
    [("alert", "error"), ("danger", "error"), ("warning", "warn"), ("ok", "success")],
)
def test_every_name_for_a_severity_lands_on_one_look(alias: str, canonical: str) -> None:
    assert notice_tone(alias) is notice_tone(canonical)


def test_information_is_not_dressed_as_success() -> None:
    """The bell used to draw an "info" notice with the success tick, in success green."""
    info, success = notice_tone("info"), notice_tone("success")
    assert info.icon != success.icon
    assert info.token != success.token


def test_the_toast_and_the_bell_agree(qapp) -> None:
    from private_ai.ui.widgets.toast import Toast

    theme.apply_theme(qapp, "light", "normal")
    for tone in ("success", "info", "warn", "error"):
        spec = notice_tone(tone)
        toast = Toast(NOTICE, tone, "")
        assert toast._tone_color == theme.token(spec.token)
        toast.deleteLater()


def test_an_unknown_tone_reads_as_information(overlay) -> None:
    overlay.show_toast("Có gì đó vừa xảy ra", "khong-biet")
    assert overlay._live[0]._tone_color == theme.token("accent")


# --- the notice row --------------------------------------------------------


def test_a_notice_row_styles_none_of_its_children() -> None:
    """A stylesheet on the row is the bracket bug; the marker is painted instead."""
    from PySide6.QtWidgets import QWidget

    row = build_notice()
    assert row.styleSheet() == ""
    styled = [
        f"{type(child).__name__}: {child.styleSheet()}"
        for child in row.findChildren(QWidget)
        if child.styleSheet() and "color:" not in child.styleSheet()
    ]
    assert not styled, f"a rule here reaches every QLabel below it: {styled}"


def test_only_an_unread_row_carries_the_accent_bar() -> None:
    unread = build_notice(unread=True).grab().toImage()
    read = build_notice(unread=False).grab().toImage()
    accent = theme.token("accent")

    assert unread.pixelColor(0, unread.height() // 2).name() == accent
    assert read.pixelColor(0, read.height() // 2).name() != accent


def test_the_panel_counts_what_is_unread(qapp) -> None:
    from private_ai.ui.widgets.notifications import Notice, NotificationsPanel

    panel = NotificationsPanel()
    notices = [Notice(id=str(index), title=f"Thông báo {index}") for index in range(3)]
    panel.set_notices(notices, lambda notice: notice.id in {"0", "2"})

    assert panel._count.text() == "2 mục mới"
    panel.set_notices(notices, lambda _notice: False)
    assert panel._count.text() == ""
    panel.deleteLater()


# --- the toast card --------------------------------------------------------


def test_a_toast_without_a_title_is_one_line(overlay) -> None:
    """The generic heading said what the icon says; only a caller's title earns a row."""
    from PySide6.QtWidgets import QLabel

    overlay.show_toast("Đã lưu không gian làm việc", "success")
    plain = overlay._live[0]
    assert not [
        label for label in plain.findChildren(QLabel) if label.property("class") == "card-title"
    ]

    overlay.show_toast("Đã tải xong 12 tài liệu", "success", title="Kho tri thức")
    headings = [
        label.text()
        for label in overlay._live[1].findChildren(QLabel)
        if label.property("class") == "card-title"
    ]
    assert headings == ["Kho tri thức"]


def test_dismissing_a_toast_warns_about_nothing(overlay, qapp) -> None:
    """``finished.disconnect()`` on a signal with nothing on it is a warning, not an error.

    libpyside reports it through ``warnings``, so the ``suppress`` that used to guard the
    call caught nothing and every dismissed toast printed into the app's log.
    """
    import warnings

    overlay.show_toast(NOTICE, "info")
    toast = overlay._live[0]

    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        toast._dismiss()
        toast._dismiss()  # the close button, clicked again mid-fade
        settle(qapp, lambda: not overlay._live)

    assert [str(warning.message) for warning in caught] == []
    assert overlay._live == []


def test_the_countdown_runs_down_and_pauses(overlay, qapp) -> None:
    """The line is the only thing that says why a toast is about to disappear."""
    overlay.show_toast(NOTICE, "info")
    toast = overlay._live[0]
    toast.repaint()
    assert toast._remaining == pytest.approx(1.0, abs=0.05)

    settle(qapp, lambda: toast._remaining < 0.97, timeout=1.5)
    running = toast._remaining
    assert running < 1.0

    toast._timer.stop()  # what hovering does
    settle(qapp, lambda: False, timeout=0.2)
    toast.repaint()
    assert toast._remaining == running, "a paused toast must not keep counting down"
