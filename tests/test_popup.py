"""Popups must be only their card.

A ``Qt.Popup`` is a window, and the platform fills that window's rectangle before Qt paints
into it — so a card with rounded corners came out rounded *inside* a square block of
surface colour. Every popup in the app carried that spare square: the notifications panel,
both pickers and the profile menu.

The fix is a translucent window plus a painted card, and both halves are asserted here: an
attribute alone would leave the popup invisible, because a translucent top-level does not
pick its background up from the application stylesheet at all.
"""

from __future__ import annotations

import pytest

from private_ai.ui import theme

pytestmark = pytest.mark.usefixtures("qapp")

POPUPS = (
    ("private_ai.ui.widgets.notifications", "NotificationsPanel"),
    ("private_ai.ui.widgets.model_picker", "_Popup"),
    ("private_ai.ui.widgets.strategy_picker", "_Popup"),
)


def build(path: str, name: str):
    import importlib

    return getattr(importlib.import_module(path), name)()


@pytest.mark.parametrize(("path", "name"), POPUPS)
def test_a_popup_window_is_translucent(qapp, path: str, name: str) -> None:
    from PySide6.QtCore import Qt

    theme.apply_theme(qapp, "light", "normal")
    popup = build(path, name)
    assert popup.isWindow(), "a popup Qt does not treat as a window needs no translucency"
    assert popup.testAttribute(Qt.WidgetAttribute.WA_TranslucentBackground), (
        "without this the platform paints a square behind the rounded card"
    )
    popup.deleteLater()


@pytest.mark.parametrize("name", ["light", "dark"])
def test_a_popup_paints_its_own_card(qapp, name: str) -> None:
    """The corners must come out empty and the middle must come out surface-coloured."""
    from private_ai.ui.widgets.popup import RoundedPopup

    theme.apply_theme(qapp, name, "normal")
    popup = RoundedPopup()
    popup.resize(200, 120)
    image = popup.grab().toImage()

    middle = image.pixelColor(image.width() // 2, image.height() // 2)
    assert middle.name() == theme.token("surface")
    assert middle.alpha() == 255

    # One pixel in from the very corner, well inside the radius that is cut away.
    corner = image.pixelColor(1, 1)
    assert corner.alpha() == 0, "the corner still carries a square of window background"
    popup.deleteLater()


def test_the_card_radius_is_one_number(qapp) -> None:
    """The painted popup and the stylesheet must round by the same amount."""
    theme.apply_theme(qapp, "light", "normal")
    assert f"border-radius: {theme.CARD_RADIUS}px" in theme.build_qss(theme.tokens("light"))


def test_a_dropdown_draws_one_border(qapp) -> None:
    """The list inside a combo popup sits in a framed window; two borders read as a seam."""
    qss = theme.build_qss(theme.tokens("light"))
    rule = qss.split("QComboBox QAbstractItemView {", 1)[1].split("}", 1)[0]
    assert "border: 0" in rule, f"the platform already frames this window: {rule}"
