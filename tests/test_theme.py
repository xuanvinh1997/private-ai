"""The palette and the type ladder are load-bearing, so they get asserted rather than eyeballed.

A colour token is easy to nudge for looks and hard to notice when the nudge drops a label
below the contrast floor, which is exactly what had happened to ``faint`` and
``line-strong`` before this file existed.
"""

from __future__ import annotations

import re

import pytest

from private_ai.ui.theme import DARK, LIGHT, ROOT_PX, TYPE_SCALE, build_qss, type_scale

# ``accent-soft`` is deliberately absent: it only ever carries ``accent-ink``, never a gray.
SURFACES = ("bg", "surface", "surface-soft", "sidebar", "surface-hover")
TEXT_ROLES = ("ink", "text", "muted", "faint")

PALETTES = {"light": LIGHT, "dark": DARK}


def _channel(value: int) -> float:
    fraction = value / 255
    return fraction / 12.92 if fraction <= 0.04045 else ((fraction + 0.055) / 1.055) ** 2.4


def _luminance(hex_colour: str) -> float:
    digits = hex_colour.lstrip("#")
    red, green, blue = (int(digits[index : index + 2], 16) for index in (0, 2, 4))
    return 0.2126 * _channel(red) + 0.7152 * _channel(green) + 0.0722 * _channel(blue)


def contrast(foreground: str, background: str) -> float:
    """WCAG 2.1 contrast ratio, 1.0 (identical) to 21.0 (black on white)."""
    lighter, darker = sorted((_luminance(foreground), _luminance(background)), reverse=True)
    return (lighter + 0.05) / (darker + 0.05)


@pytest.mark.parametrize("theme", sorted(PALETTES))
@pytest.mark.parametrize("role", TEXT_ROLES)
@pytest.mark.parametrize("surface", SURFACES)
def test_text_roles_clear_wcag_aa(theme: str, role: str, surface: str) -> None:
    palette = PALETTES[theme]
    ratio = contrast(palette[role], palette[surface])
    assert ratio >= 4.5, f"{theme}: {role} on {surface} is {ratio:.2f}:1"


@pytest.mark.parametrize("theme", sorted(PALETTES))
def test_control_edges_are_visible(theme: str) -> None:
    """``line-strong`` is the only thing marking the edge of an input — 1.4.11 wants 3:1."""
    palette = PALETTES[theme]
    for surface in ("surface", "bg"):
        ratio = contrast(palette["line-strong"], palette[surface])
        assert ratio >= 2.8, f"{theme}: line-strong on {surface} is {ratio:.2f}:1"


@pytest.mark.parametrize("theme", sorted(PALETTES))
def test_accent_and_status_pairs_are_legible(theme: str) -> None:
    palette = PALETTES[theme]
    pairs = (
        ("on-accent", "accent"),
        ("accent-ink", "accent-soft"),
        ("danger", "danger-soft"),
        ("warn", "warn-soft"),
        ("success", "success-soft"),
    )
    for foreground, background in pairs:
        ratio = contrast(palette[foreground], palette[background])
        assert ratio >= 4.5, f"{theme}: {foreground} on {background} is {ratio:.2f}:1"


def test_both_themes_define_the_same_tokens() -> None:
    assert set(LIGHT) == set(DARK)


@pytest.mark.parametrize("scale", sorted(ROOT_PX))
def test_type_ladder_is_monotonic_and_readable(scale: str) -> None:
    sizes = type_scale(scale)
    ordered = [sizes[key] for key in TYPE_SCALE]
    assert ordered == sorted(ordered), f"{scale}: ladder is not ascending: {sizes}"
    assert min(ordered) >= 11, f"{scale}: {min(ordered)}px is below the readable floor"
    # A step the eye cannot resolve is not a step; the two smallest may collide when the
    # root shrinks, but nothing above the body size is allowed to.
    body_up = [key for key in TYPE_SCALE if sizes[key] >= sizes["base"]]
    for earlier, later in zip(body_up, body_up[1:], strict=False):
        collision = f"{scale}: {earlier} and {later} are both {sizes[later]}px"
        assert sizes[later] > sizes[earlier], collision


@pytest.mark.parametrize("theme", sorted(PALETTES))
@pytest.mark.parametrize("scale", sorted(ROOT_PX))
def test_stylesheet_renders_with_every_token_substituted(theme: str, scale: str) -> None:
    qss = build_qss(PALETTES[theme], scale)
    # A rendered rule reads ``{ color: #17231f; }``; a missed token reads ``{ink}``.
    leftover = re.findall(r"\{[a-z_][a-z_0-9]*\}", qss)
    assert not leftover, f"unsubstituted placeholders: {sorted(set(leftover))}"
    assert qss.count("QLabel[class=") >= 8


# --- painted geometry ------------------------------------------------------
#
# The stylesheet declares 30px and Qt paints 32px, because ``min-height`` is measured
# against the content rect and the 1px border is added on each side. That arithmetic is
# easy to get wrong by eye — the first draft of this palette declared 32px with 6px of
# vertical padding and painted 46px buttons next to 42px inputs — so the numbers are
# asserted against real laid-out widgets rather than read off the QSS.

CONTROL_CLASSES = (
    ("QPushButton", None),
    ("QPushButton", "primary"),
    ("QPushButton", "danger"),
    ("QPushButton", "icon"),
    ("QLineEdit", None),
    ("QSpinBox", None),
    ("QComboBox", None),
    ("QToolButton", None),
)

BADGE_CLASSES = ("pill", "chip", "chip-active", "badge-success", "badge-warn", "badge-danger")


def _laid_out(qapp, widgets, factories):
    """Build one row of widgets and let the layout settle, so heights are real."""
    host = widgets.QWidget()
    row = widgets.QHBoxLayout(host)
    row.setContentsMargins(0, 0, 0, 0)
    built = []
    for factory in factories:
        widget = factory(widgets)
        row.addWidget(widget)
        built.append(widget)
    host.resize(1400, 120)
    host.show()
    qapp.processEvents()
    return host, built


def test_every_control_lands_on_one_baseline(qapp) -> None:
    from PySide6 import QtWidgets as widgets

    from private_ai.ui.theme import CONTROL_HEIGHT, apply_theme

    apply_theme(qapp, "light", "normal")

    def make(name: str, css_class: str | None):
        def factory(module):
            widget = getattr(module, name)()
            if hasattr(widget, "setText"):
                widget.setText("Tài liệu")
            if css_class:
                widget.setProperty("class", css_class)
            return widget

        return factory

    host, built = _laid_out(qapp, widgets, [make(*case) for case in CONTROL_CLASSES])
    try:
        heights = {
            f"{name}[{css_class}]": widget.height()
            for (name, css_class), widget in zip(CONTROL_CLASSES, built, strict=True)
        }
        off = {key: value for key, value in heights.items() if value != CONTROL_HEIGHT}
        assert not off, f"controls off the {CONTROL_HEIGHT}px baseline: {off}"
    finally:
        host.close()


def test_badges_share_one_height_and_do_not_stretch(qapp) -> None:
    """A badge with no max-height fills whatever row it lands in, which looks like a bug."""
    from PySide6 import QtWidgets as widgets

    from private_ai.ui.theme import apply_theme

    apply_theme(qapp, "light", "normal")

    def make(css_class: str):
        def factory(module):
            label = module.QLabel("Sẵn sàng")
            label.setProperty("class", css_class)
            return label

        return factory

    host, built = _laid_out(qapp, widgets, [make(name) for name in BADGE_CLASSES])
    try:
        heights = {name: w.height() for name, w in zip(BADGE_CLASSES, built, strict=True)}
        assert set(heights.values()) == {28}, f"badges disagree on height: {heights}"
    finally:
        host.close()


def test_line_edit_side_icons_stay_on_the_field_centre(qapp) -> None:
    """A search glyph half a field below the text reads as a rendering fault, not a control.

    Qt lays a ``QLineEdit``'s leading/clear buttons out at the stock side-widget size and
    centres them on that assumption, so the toolbar ``QToolButton`` metrics leaking in
    push the glyph down by the difference instead of growing the field.
    """
    from PySide6 import QtWidgets as widgets

    from private_ai.ui.icons import icon
    from private_ai.ui.theme import apply_theme

    apply_theme(qapp, "light", "normal")

    def factory(module):
        field = module.QLineEdit()
        field.setClearButtonEnabled(True)
        field.setPlaceholderText("Tìm theo tên tệp")
        field.addAction(icon("search"), module.QLineEdit.ActionPosition.LeadingPosition)
        return field

    host, (field,) = _laid_out(qapp, widgets, [factory])
    try:
        buttons = field.findChildren(widgets.QToolButton)
        assert buttons, "the line edit grew no side widgets to measure"
        off = {
            f"{button.geometry()}": button.geometry().center().y()
            for button in buttons
            if abs(button.geometry().center().y() - field.rect().center().y()) > 1
            or button.height() > field.height()
        }
        assert not off, f"side icons off the field centre (field {field.rect()}): {off}"
    finally:
        host.close()
