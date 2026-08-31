"""A toast must be as tall as the message it was given.

Qt does not apply ``heightForWidth`` to a layout item that carries an alignment flag, so
the toasts — added with ``AlignRight`` — were laid out at their unwrapped height and cut
their own second line off. The overlay now measures each one at the width it will actually
be painted at, and this holds it to that: no label may end below its toast's bottom edge,
and no toast may be taller than the text needs.
"""

from __future__ import annotations

import pytest

SHORT = "Ngắn."
ONE_LINE = "Không nghe được gì. Kiểm tra micro rồi ghi lại."
WRAPPING = (
    "Không đọc được thư viện mô hình. Khởi động nhà cung cấp rồi thử lại, hoặc kiểm tra "
    "lại địa chỉ máy chủ trong phần Cài đặt của ứng dụng."
)


@pytest.fixture
def overlay(qapp):
    from PySide6.QtWidgets import QWidget

    from private_ai.ui import theme
    from private_ai.ui.widgets.toast import ToastOverlay

    theme.apply_theme(qapp, "light", "normal")
    host = QWidget()
    host.resize(1280, 860)
    host.show()
    made = ToastOverlay(host)
    yield made
    host.close()


def _labels(toast):
    from PySide6.QtWidgets import QLabel

    return [label for label in toast.findChildren(QLabel) if (label.text() or "").strip()]


@pytest.mark.parametrize("message", [SHORT, ONE_LINE, WRAPPING])
def test_the_message_fits_inside_its_toast(overlay, qapp, message: str) -> None:
    overlay.show_toast(message, "info")
    qapp.processEvents()
    qapp.processEvents()
    toast = overlay._live[-1]
    overflowing = [
        f"{label.text()[:32]!r} ends at {label.mapTo(toast, label.rect().bottomLeft()).y()}"
        for label in _labels(toast)
        if label.mapTo(toast, label.rect().bottomLeft()).y() > toast.height()
    ]
    assert not overflowing, f"toast is {toast.height()}px and clips: " + "; ".join(overflowing)


def test_a_short_toast_is_not_padded_out(overlay, qapp) -> None:
    """The height comes from heightForWidth, not from sizeHint.

    sizeHint answers for the width the message would like — one long unwrapped line — so
    using it gave a short toast twenty pixels of dead air that its two labels then shared.
    """
    overlay.show_toast(SHORT, "info")
    overlay.show_toast(ONE_LINE, "info")
    qapp.processEvents()
    qapp.processEvents()
    short, one_line = overlay._live
    assert short.width() == one_line.width(), "toasts are one column, not a ragged stack"
    assert short.height() == one_line.height(), (
        "two messages that each fit on one line must produce two toasts of one height: "
        f"{short.height()} vs {one_line.height()}"
    )


def test_the_overlay_is_tall_enough_for_its_stack(overlay, qapp) -> None:
    for message in (SHORT, ONE_LINE, WRAPPING):
        overlay.show_toast(message, "info")
    qapp.processEvents()
    qapp.processEvents()
    spacing = overlay._layout.spacing()
    stacked = sum(toast.height() for toast in overlay._live)
    stacked += spacing * (len(overlay._live) - 1)
    assert overlay.height() >= stacked, (
        f"overlay is {overlay.height()}px for a {stacked}px stack of {len(overlay._live)}"
    )
