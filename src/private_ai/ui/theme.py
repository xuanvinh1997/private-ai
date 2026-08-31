"""Design tokens ported from ``apps/web/src/styles.css``, rendered as a Qt stylesheet.

The web app had one ladder of colours and one ladder of font sizes; keeping the same two
ladders here is what makes the Qt port look like the same product rather than a
lookalike. Every token below is the literal value from the CSS, so a change over there is
a one-line change here.

Switching theme is deliberately a single ``app.setStyleSheet(build_qss(...))`` call: Qt
re-polishes the whole tree for us, and nothing has to remember which widgets it painted.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from PySide6.QtWidgets import QApplication, QWidget

logger = logging.getLogger("private_ai.ui.theme")

__all__ = [
    "BADGE_HEIGHT",
    "CARD_MARGINS",
    "CARD_SPACING",
    "CONTROL_HEIGHT",
    "DARK",
    "DIALOG_MARGINS",
    "DIALOG_SPACING",
    "GRAPH_PALETTE",
    "LIGHT",
    "PAGE_MARGINS",
    "PAGE_SPACING",
    "SPACE",
    "TOOLBAR_SPACING",
    "TYPE_SCALE",
    "apply_theme",
    "build_qss",
    "current_theme",
    "font_scale_px",
    "graph_palette",
    "load_bundled_fonts",
    "resolve_theme_name",
    "restyle",
    "space",
    "token",
    "tokens",
    "type_scale",
]

# The 19 CSS custom properties, plus five the web app expressed with color-mix() and Qt
# cannot compute at runtime. Four grays deviate from the CSS on purpose: ``muted`` and
# ``faint`` were lifted until the smallest text that uses them clears WCAG AA (4.5:1) on
# every surface they land on, and ``line-strong`` was darkened to ~3:1 because it is the
# only thing that marks the edge of an input — the CSS values were 2.9:1 and 1.6:1.
LIGHT: dict[str, str] = {
    "bg": "#f3f6f4",
    "sidebar": "#edf2ef",
    "surface": "#ffffff",
    "surface-soft": "#f7f9f8",
    "surface-hover": "#e6ede9",
    "ink": "#17231f",
    "text": "#293732",
    "muted": "#55635d",
    "faint": "#5f6e69",
    "line": "#d8e0dc",
    "line-strong": "#87968f",
    "accent": "#176b59",
    "accent-hover": "#115747",
    "accent-soft": "#deeee8",
    "accent-ink": "#0c4d3f",
    "danger": "#ad403c",
    "danger-soft": "#f8e5e3",
    "shadow": "rgba(36, 58, 50, 0.12)",
    "scrim": "rgba(19, 31, 27, 0.42)",
    # Derived — the web app mixed these inline; Qt stylesheets have no color-mix().
    "success": "#176b59",
    "success-soft": "#deeee8",
    "warn": "#8a5a12",
    "warn-soft": "#f6ecd8",
    "on-accent": "#ffffff",
}

DARK: dict[str, str] = {
    "bg": "#0b1412",
    "sidebar": "#0f1b18",
    "surface": "#13221e",
    "surface-soft": "#172923",
    "surface-hover": "#1d332c",
    "ink": "#e6efeb",
    "text": "#d2ded9",
    "muted": "#9aaba4",
    "faint": "#8a9c94",
    "line": "#294039",
    "line-strong": "#58776d",
    "accent": "#66c4aa",
    "accent-hover": "#7dd1b8",
    "accent-soft": "#173b31",
    "accent-ink": "#b9eadc",
    "danger": "#ee8b83",
    "danger-soft": "#432725",
    "shadow": "rgba(0, 0, 0, 0.28)",
    "scrim": "rgba(0, 0, 0, 0.58)",
    "success": "#66c4aa",
    "success-soft": "#173b31",
    "warn": "#e0b562",
    "warn-soft": "#39301c",
    "on-accent": "#08201a",
}

THEMES: dict[str, dict[str, str]] = {"light": LIGHT, "dark": DARK}

# Categorical *data* colours, not interface colours: an entity type is hashed into this
# tuple so the same type keeps its hue across sessions, and the interface ramp has no eight
# distinguishable slots to borrow. Two sets, because eight hues dark enough to read on a
# white canvas are the eight that disappear into a near-black one. Every entry clears 3:1
# against its own background, which is what makes a node read as a shape.
GRAPH_PALETTE: dict[str, tuple[str, ...]] = {
    "light": (
        "#1c7a63",
        "#3d6fb4",
        "#a8672c",
        "#7d55ab",
        "#a8465c",
        "#2f8f8a",
        "#5c6f3a",
        "#8a5a86",
    ),
    "dark": (
        "#5fc4a8",
        "#7fb0e8",
        "#e0a86a",
        "#b79ae6",
        "#e58fa4",
        "#69c6c0",
        "#a6bd72",
        "#cf9ac9",
    ),
}

# The CSS ladder in rem. Multiplied by the root size to get px, so "large text" is one
# number change and nothing lands below ~11px.
TYPE_SCALE: dict[str, float] = {
    "2xs": 0.74,
    "xs": 0.80,
    "sm": 0.87,
    "base": 0.93,
    "md": 1.00,
    "lg": 1.18,
    "xl": 1.42,
    "2xl": 1.73,
    "display": 2.13,
}

ROOT_PX: dict[str, int] = {"compact": 14, "normal": 15, "large": 18}

# One 4px grid for every margin, gap and pad in the app. Layout code asks for a step by
# name, so a row of widgets built in three different files still lines up.
SPACE: dict[str, int] = {
    "3xs": 2,
    "2xs": 4,
    "xs": 6,
    "sm": 8,
    "md": 12,
    "lg": 16,
    "xl": 20,
    "2xl": 24,
    "3xl": 32,
    "4xl": 40,
}

# The handful of composites every screen repeats. Named here rather than retyped, which
# is what let the old code drift to eleven different page paddings.
PAGE_MARGINS: tuple[int, int, int, int] = (24, 20, 24, 20)
PAGE_SPACING = SPACE["lg"]
CARD_MARGINS: tuple[int, int, int, int] = (16, 12, 16, 12)
CARD_SPACING = SPACE["md"]
DIALOG_MARGINS: tuple[int, int, int, int] = (24, 20, 24, 20)
DIALOG_SPACING = SPACE["md"]
TOOLBAR_SPACING = SPACE["sm"]
# The painted height every interactive control resolves to, so a toolbar of buttons, inputs
# and combo boxes has one baseline instead of three. The stylesheet declares 30px because Qt
# measures ``min-height`` against the content rect and adds the 1px border on each side;
# ``tests/test_theme.py`` measures the real widgets and holds them to this number.
CONTROL_HEIGHT = 32
# Status labels are a shape, not a control: they sit *beside* 32px controls in the same
# row, so they get their own smaller height. Any row that shows the badge on some cards
# and not others must reserve this much, or the cards land on two different baselines.
BADGE_HEIGHT = 26

UI_FONTS = '"Manrope", "Manrope Variable", "Segoe UI", "Helvetica Neue", sans-serif'
MONO_FONTS = '"IBM Plex Mono", "SF Mono", "Cascadia Mono", "Consolas", monospace'

# The two families the type ladder is drawn for, shipped with the app. Neither is a system
# font anywhere, so without these the whole scale silently renders in Helvetica and every
# measurement in ``tests/test_theme.py`` describes a font the user never sees.
FONT_DIR = Path(__file__).resolve().parent / "assets" / "fonts"

_current: dict[str, str] = {"theme": "light", "font_scale": "normal"}
_fonts_loaded = False


def load_bundled_fonts() -> list[str]:
    """Register the shipped font files with Qt. Idempotent; safe before ``QApplication``
    exists only in the sense that it will do nothing and say so."""
    global _fonts_loaded
    if _fonts_loaded:
        return []
    from PySide6.QtGui import QFontDatabase

    families: list[str] = []
    for path in sorted(FONT_DIR.glob("*.ttf")):
        # Qt resolves nothing here: a relative path fails silently with -1 on macOS.
        handle = QFontDatabase.addApplicationFont(str(path.resolve()))
        if handle < 0:
            logger.warning("không nạp được font %s", path.name)
            continue
        families.extend(QFontDatabase.applicationFontFamilies(handle))
    _fonts_loaded = True
    if families:
        logger.debug("đã nạp font: %s", ", ".join(sorted(set(families))))
    return families


def resolve_theme_name(name: str) -> str:
    """``"system"`` is a preference, not a palette — collapse it to a real one."""
    candidate = (name or "").strip().lower()
    if candidate in THEMES:
        return candidate
    if candidate == "system":
        return _system_theme()
    return "light"


def _system_theme() -> str:
    try:
        from PySide6.QtCore import Qt
        from PySide6.QtGui import QGuiApplication

        hints = QGuiApplication.styleHints()
        scheme = getattr(hints, "colorScheme", None)
        if scheme is not None and scheme() == Qt.ColorScheme.Dark:
            return "dark"
    except Exception:  # pragma: no cover - no Qt, or a Qt too old for colorScheme()
        pass
    return "light"


def current_theme() -> str:
    return _current["theme"]


def current_font_scale() -> str:
    return _current["font_scale"]


def tokens(name: str | None = None) -> dict[str, str]:
    """The palette dict. Copied, so a caller cannot poison the module-level table."""
    return dict(THEMES[resolve_theme_name(name or _current["theme"])])


def token(key: str, name: str | None = None) -> str:
    palette = THEMES[resolve_theme_name(name or _current["theme"])]
    value = palette.get(key)
    if value is None:
        logger.warning("Token giao diện không tồn tại: %s", key)
        return palette["text"]
    return value


def graph_palette(name: str | None = None) -> tuple[str, ...]:
    """The eight data hues for the active theme. Read at paint time, like every token."""
    return GRAPH_PALETTE[resolve_theme_name(name or _current["theme"])]


def font_scale_px(scale: str) -> int:
    return ROOT_PX.get(scale, ROOT_PX["normal"])


def type_scale(scale: str = "") -> dict[str, int]:
    """The rem ladder resolved to whole pixels for the given root size."""
    root = font_scale_px(scale or _current["font_scale"])
    return {key: max(11, round(root * ratio)) for key, ratio in TYPE_SCALE.items()}


def space(*keys: str) -> tuple[int, ...] | int:
    """Look up one or more steps on the 4px grid: ``space("lg")`` or ``space("lg", "md")``."""
    values = tuple(SPACE[key] for key in keys)
    return values[0] if len(values) == 1 else values


def restyle(widget: QWidget) -> None:
    """Re-evaluate stylesheet rules after a dynamic property changed.

    Qt caches the computed style per widget; changing ``class`` after the widget is shown
    does nothing until the style is unpolished and polished again.
    """
    style = widget.style()
    style.unpolish(widget)
    style.polish(widget)
    widget.update()


def _subcontrol_qss(tk: dict[str, str]) -> str:
    """The parts of a control Qt will only draw from an image file.

    A spin box's steppers, a combo box's chevron and a check box's tick are sub-controls:
    style the widget and Qt still paints these from the *native* style unless an ``image``
    says otherwise. That is how a light theme ended up with system arrows straddling its
    own rounded borders. Without a running QGuiApplication there is nothing to rasterise
    with, so the block comes back empty and Qt keeps its own drawing.
    """
    from private_ai.ui import icons

    chevron_down = icons.icon_path("chevron-down", tk["muted"], 12)
    chevron_up = icons.icon_path("chevron-up", tk["muted"], 12)
    tick = icons.icon_path("check", tk["on-accent"], 12)
    if not (chevron_down and chevron_up and tick):
        return ""
    return f"""
/* ---------- sub-controls drawn from images ---------- */
QComboBox::drop-down {{
    subcontrol-origin: padding;
    subcontrol-position: center right;
    width: 26px;
    border: 0;
    background: transparent;
}}
QComboBox::down-arrow {{ image: url({chevron_down}); width: 12px; height: 12px; }}
QComboBox::down-arrow:disabled {{ image: none; }}

/* The steppers are stacked inside the border, each half the field's painted height, so
   they land inside the corner radius instead of across it. */
QSpinBox, QDoubleSpinBox {{ padding-right: 22px; }}
QSpinBox::up-button, QDoubleSpinBox::up-button,
QSpinBox::down-button, QDoubleSpinBox::down-button {{
    subcontrol-origin: border;
    width: 20px;
    height: 14px;
    border: 0;
    background: transparent;
}}
QSpinBox::up-button, QDoubleSpinBox::up-button {{
    subcontrol-position: top right;
    margin: 1px 1px 0 0;
    border-top-right-radius: 9px;
}}
QSpinBox::down-button, QDoubleSpinBox::down-button {{
    subcontrol-position: bottom right;
    margin: 0 1px 1px 0;
    border-bottom-right-radius: 9px;
}}
QSpinBox::up-button:hover, QDoubleSpinBox::up-button:hover,
QSpinBox::down-button:hover, QDoubleSpinBox::down-button:hover {{
    background: {tk["surface-hover"]};
}}
QSpinBox::up-arrow, QDoubleSpinBox::up-arrow {{
    image: url({chevron_up});
    width: 10px;
    height: 10px;
}}
QSpinBox::down-arrow, QDoubleSpinBox::down-arrow {{
    image: url({chevron_down});
    width: 10px;
    height: 10px;
}}
QSpinBox::up-arrow:off, QSpinBox::down-arrow:off,
QDoubleSpinBox::up-arrow:off, QDoubleSpinBox::down-arrow:off {{ image: none; }}

/* Without this the checked box is a filled square with nothing in it: styling the
   indicator suppresses the tick the native style would have drawn. */
QCheckBox::indicator:checked {{ image: url({tick}); }}
"""


def build_qss(tk: dict[str, str], font_scale: str = "normal") -> str:
    """Render the whole application stylesheet by substitution.

    One string for the entire app: Qt applies it top-down and every widget that sets a
    ``class`` property or a known objectName picks up its rule without any per-widget
    styling code.
    """
    t = dict(tk)
    fs = type_scale(font_scale)
    root = font_scale_px(font_scale)
    return _TEMPLATE.format(
        subcontrols=_subcontrol_qss(t),
        ui=UI_FONTS,
        mono=MONO_FONTS,
        root=root,
        badge_h=BADGE_HEIGHT,
        f2xs=fs["2xs"],
        fxs=fs["xs"],
        fsm=fs["sm"],
        fbase=fs["base"],
        fmd=fs["md"],
        flg=fs["lg"],
        fxl=fs["xl"],
        f2xl=fs["2xl"],
        fdisplay=fs["display"],
        **{key.replace("-", "_"): value for key, value in t.items()},
    )


def apply_theme(
    app: QApplication,
    name: str = "light",
    font_scale: str = "normal",
) -> dict[str, str]:
    """Apply a palette to the running application and return the tokens it used."""
    resolved = resolve_theme_name(name)
    scale = font_scale if font_scale in ROOT_PX else "normal"
    _current["theme"] = resolved
    _current["font_scale"] = scale
    tk = THEMES[resolved]

    from PySide6.QtGui import QColor, QFont, QPalette

    # Before the first QFont is built: a family Qt has not been told about resolves to the
    # platform default and never comes back, even if the file is registered a line later.
    load_bundled_fonts()

    font = QFont()
    font.setPixelSize(font_scale_px(scale))
    font.setFamilies(["Manrope", "Manrope Variable", "Segoe UI", "Helvetica Neue"])
    app.setFont(font)

    # QSS does not reach every native-drawn surface (tooltips, some scroll areas), so the
    # QPalette is set as well and the two agree.
    palette = QPalette()
    palette.setColor(QPalette.ColorRole.Window, QColor(tk["bg"]))
    palette.setColor(QPalette.ColorRole.WindowText, QColor(tk["text"]))
    palette.setColor(QPalette.ColorRole.Base, QColor(tk["surface"]))
    palette.setColor(QPalette.ColorRole.AlternateBase, QColor(tk["surface-soft"]))
    palette.setColor(QPalette.ColorRole.Text, QColor(tk["text"]))
    palette.setColor(QPalette.ColorRole.Button, QColor(tk["surface"]))
    palette.setColor(QPalette.ColorRole.ButtonText, QColor(tk["text"]))
    palette.setColor(QPalette.ColorRole.Highlight, QColor(tk["accent"]))
    palette.setColor(QPalette.ColorRole.HighlightedText, QColor(tk["on-accent"]))
    palette.setColor(QPalette.ColorRole.ToolTipBase, QColor(tk["surface"]))
    palette.setColor(QPalette.ColorRole.ToolTipText, QColor(tk["text"]))
    palette.setColor(QPalette.ColorRole.PlaceholderText, QColor(tk["faint"]))
    palette.setColor(QPalette.ColorRole.Link, QColor(tk["accent"]))
    app.setPalette(palette)

    app.setProperty("uiTheme", resolved)
    app.setStyleSheet(build_qss(tk, scale))

    # Icons are tinted with the ink colour, so their cache has to die with the theme.
    try:
        from private_ai.ui import icons

        icons.invalidate_cache()
    except Exception:  # pragma: no cover - icons module is optional at this point
        logger.debug("Không làm mới được bộ nhớ đệm biểu tượng", exc_info=True)

    return dict(tk)


# ``{{`` / ``}}`` are literal braces; every ``{name}`` is a token or a font size.
_TEMPLATE = """
* {{
    font-family: {ui};
    font-size: {fbase}px;
    outline: none;
}}
QWidget {{
    color: {text};
    background: transparent;
}}
QMainWindow, QDialog, #MainStage {{
    background: {bg};
}}
QToolTip {{
    color: {text};
    background: {surface};
    border: 1px solid {line};
    border-radius: 6px;
    padding: 5px 8px;
}}

/* ---------- typography helpers, selected with the `class` property ---------- */
QLabel[class="display"] {{ color: {ink}; font-size: {fdisplay}px; font-weight: 760; }}
QLabel[class="title"] {{ color: {ink}; font-size: {fxl}px; font-weight: 700; }}
QLabel[class="heading"] {{ color: {ink}; font-size: {flg}px; font-weight: 700; }}
/* The name of a card or a list row. Every screen had been reaching for "subtitle" here,
   which is the muted 13px description step — so a provider name and its endpoint line
   rendered identically and the cards read as one flat block of gray. */
QLabel[class="card-title"] {{ color: {ink}; font-size: {fmd}px; font-weight: 700; }}
QLabel[class="body"] {{ color: {text}; font-size: {fbase}px; }}
/* The emphasised half of the body pair. Six widgets were each reaching for an inline
   ``font-weight`` sheet to get here, at four different weights. */
QLabel[class="body-strong"] {{ color: {ink}; font-size: {fbase}px; font-weight: 700; }}
QLabel[class="subtitle"] {{ color: {muted}; font-size: {fsm}px; }}
/* A checked rail row recolours its own text, but a row built out of child labels cannot
   inherit that: QLabel rules do not cascade from the button. So the accent is stated. */
QLabel[class="rail-active"] {{ color: {accent_ink}; font-size: {fbase}px; font-weight: 700; }}
QLabel[class="muted"] {{ color: {muted}; font-size: {fsm}px; }}
QLabel[class="faint"] {{ color: {faint}; font-size: {fxs}px; }}
QLabel[class="danger"] {{ color: {danger}; font-size: {fsm}px; }}
QLabel[class="empty"] {{
    color: {muted};
    font-size: {fsm}px;
    padding: 24px 0;
}}
/* No horizontal padding, deliberately: this is the eyebrow above a title, and any pad
   here shifts it off the text column every other label in the stack shares. */
QLabel[class="section-label"] {{
    color: {muted};
    font-size: {fxs}px;
    font-weight: 720;
    letter-spacing: 0.4px;
    padding: 0;
}}
/* Read-only status badges. The button chips below share the shape; these are the
   QLabel half, which until now fell through every rule and rendered as plain text. */
QLabel[class="pill"], QLabel[class="chip"], QLabel[class="chip-active"],
QLabel[class="badge-success"], QLabel[class="badge-warn"], QLabel[class="badge-danger"] {{
    min-height: {badge_h}px;
    max-height: {badge_h}px;
    border: 1px solid {line};
    border-radius: 999px;
    padding: 0 10px;
    color: {muted};
    background: {surface_soft};
    font-size: {f2xs}px;
    font-weight: 700;
}}
QLabel[class="chip-active"] {{
    color: {accent_ink};
    background: {accent_soft};
    border-color: {accent};
}}
QLabel[class="badge-success"] {{
    color: {success};
    background: {success_soft};
    border-color: {success_soft};
}}
QLabel[class="badge-warn"] {{
    color: {warn};
    background: {warn_soft};
    border-color: {warn_soft};
}}
QLabel[class="badge-danger"] {{
    color: {danger};
    background: {danger_soft};
    border-color: {danger_soft};
}}

QLabel[class="avatar"], QLabel[class="avatar-lg"] {{
    min-width: 28px;
    max-width: 28px;
    min-height: 28px;
    max-height: 28px;
    border-radius: 14px;
    color: {accent_ink};
    background: {accent_soft};
    font-family: {mono};
    font-size: {f2xs}px;
    font-weight: 700;
}}

QLabel[class="avatar-lg"] {{
    min-width: 32px;
    max-width: 32px;
    min-height: 32px;
    max-height: 32px;
    border-radius: 16px;
    font-size: {fsm}px;
}}

QLabel[class="code"], QPlainTextEdit[class="code"] {{
    font-family: {mono};
    font-size: {fsm}px;
    color: {ink};
    background: {surface_soft};
    border: 1px solid {line};
    border-radius: 8px;
    padding: 8px 10px;
}}

/* ---------- buttons ---------- */
QPushButton {{
    min-height: 30px;
    max-height: 30px;
    border: 1px solid {line_strong};
    border-radius: 9px;
    padding: 0 14px;
    color: {text};
    background: {surface};
    font-weight: 600;
}}
QPushButton:hover {{ background: {surface_hover}; border-color: {line_strong}; }}
QPushButton:pressed {{ background: {surface_hover}; }}
QPushButton:disabled {{ color: {faint}; background: {surface_soft}; border-color: {line}; }}
QPushButton:focus {{
    border-color: {accent};
    background: {accent_soft};
    color: {accent_ink};
}}
QPushButton[class="primary"]:focus {{
    border-color: {accent_ink};
    background: {accent};
    color: {on_accent};
}}

QPushButton[class="primary"] {{
    color: {on_accent};
    background: {accent};
    border-color: {accent};
    font-weight: 700;
}}
QPushButton[class="primary"]:hover {{ background: {accent_hover}; border-color: {accent_hover}; }}
QPushButton[class="primary"]:disabled {{
    color: {faint};
    background: {surface_soft};
    border-color: {line};
}}
QPushButton[class="cta"] {{
    min-height: 38px;
    max-height: 38px;
    border-radius: 10px;
    padding: 0 14px;
    color: {on_accent};
    background: {accent};
    border: 1px solid {accent};
    font-weight: 700;
}}
QPushButton[class="cta"]:hover {{ background: {accent_hover}; border-color: {accent_hover}; }}
QPushButton[class="cta"]:focus {{ border-color: {accent_ink}; }}
QPushButton[class="cta"]:disabled {{
    color: {faint};
    background: {surface_soft};
    border-color: {line};
}}
QPushButton[class="danger"] {{
    color: {danger};
    background: {danger_soft};
    border-color: {danger_soft};
    font-weight: 700;
}}
QPushButton[class="danger"]:hover {{ border-color: {danger}; }}
QPushButton[class="ghost"] {{
    border: 1px solid transparent;
    background: transparent;
    color: {muted};
    font-weight: 580;
}}
QPushButton[class="ghost"]:hover {{ color: {ink}; background: {surface_hover}; }}
/* A row's own title that happens to be clickable. It takes card-title's weight and, more
   to the point, no horizontal padding: as a "ghost" it inherited the 14px button inset and
   every filename floated half a word right of the metadata line beneath it. */
QPushButton[class="row-title"] {{
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 0;
    background: transparent;
    color: {ink};
    font-size: {fmd}px;
    font-weight: 700;
    text-align: left;
}}
QPushButton[class="row-title"]:hover {{ color: {accent_ink}; }}
QPushButton[class="row-title"]:disabled {{ color: {muted}; }}
/* Both button classes, so "icon" means one square glyph affordance no matter which Qt
   class a row happened to build. A bare QToolButton is 30px tall but keeps side padding;
   this pins it square. */
QPushButton[class="icon"], QToolButton[class="icon"] {{
    min-width: 30px;
    max-width: 30px;
    min-height: 30px;
    max-height: 30px;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 0;
    background: transparent;
    color: {muted};
}}
QPushButton[class="icon"]:hover, QToolButton[class="icon"]:hover {{
    color: {ink};
    background: {surface_hover};
}}
QPushButton[class="icon"]:checked, QToolButton[class="icon"]:checked {{
    color: {accent};
    background: {accent_soft};
}}
QToolButton[class="chip"], QToolButton[class="chip-active"] {{
    min-height: 26px;
    max-height: 26px;
    border-radius: 999px;
    padding: 0 12px;
    color: {muted};
    background: {surface_soft};
    border: 1px solid {line};
    font-size: {fxs}px;
    font-weight: 620;
}}
QToolButton[class="chip"]:hover, QToolButton[class="chip-active"]:hover {{
    color: {ink};
    background: {surface_hover};
}}
QToolButton[class="chip"]:checked, QToolButton[class="chip-active"] {{
    color: {accent_ink};
    background: {accent_soft};
    border-color: {accent};
}}

QPushButton[class="chip"] {{
    min-height: 26px;
    max-height: 26px;
    border-radius: 999px;
    padding: 0 12px;
    color: {muted};
    background: {surface_soft};
    border: 1px solid {line};
    font-size: {fxs}px;
    font-weight: 620;
}}
QPushButton[class="chip"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="chip"]:checked, QPushButton[class="chip-active"] {{
    color: {accent_ink};
    background: {accent_soft};
    border-color: {accent};
}}
/* A true segmented control: one track, with only the choice filled inside it. Three loose
   pills made every option look equally set — the track is what says one of them is. */
QWidget[class="segment"] {{
    border: 1px solid {line};
    border-radius: 10px;
    background: {surface_soft};
}}
QPushButton[class="segment-item"] {{
    min-height: 26px;
    max-height: 26px;
    border: 0;
    border-radius: 8px;
    padding: 0 13px;
    color: {muted};
    background: transparent;
    font-size: {fxs}px;
    font-weight: 620;
}}
QPushButton[class="segment-item"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="segment-item"]:checked {{
    color: {accent_ink};
    background: {accent_soft};
    font-weight: 700;
}}
QPushButton[class="segment-item"]:disabled {{ color: {faint}; }}
QPushButton[class="menu-item"] {{
    min-height: 30px;
    max-height: 30px;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 0 10px;
    color: {text};
    background: transparent;
    text-align: left;
    font-weight: 580;
}}
QPushButton[class="menu-item"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="menu-item"]:focus {{ border-color: {accent}; }}
QPushButton[class="menu-item"]:checked {{ color: {accent_ink}; background: {accent_soft}; }}

QPushButton[class="nav-item"] {{
    min-height: 40px;
    max-height: 40px;
    border: 0;
    border-radius: 9px;
    padding: 0 12px;
    color: {muted};
    background: transparent;
    font-weight: 580;
    text-align: left;
}}
QPushButton[class="nav-item"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="nav-item"]:checked {{
    color: {accent_ink};
    background: {accent_soft};
    font-weight: 700;
}}

/* The left rail runs its own, denser row scale. Five destinations, the workspaces and the
   recents all stack in one column, so the four pixels a nav-item spends on height cost the
   rail forty — and the recents list is what pays. */
QPushButton[class="rail-item"] {{
    min-height: 36px;
    max-height: 36px;
    border: 0;
    border-radius: 9px;
    padding: 0 10px;
    color: {muted};
    background: transparent;
    font-weight: 580;
    text-align: left;
}}
QPushButton[class="rail-item"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="rail-item"]:focus {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="rail-item"]:checked {{
    color: {accent_ink};
    background: {accent_soft};
    font-weight: 700;
}}
/* Two lines of copy do not fit the single-line scale; the recents rows get the height they
   need rather than clipping their own descenders. */
QPushButton[class="rail-row"] {{
    min-height: 46px;
    max-height: 46px;
    border: 0;
    border-radius: 9px;
    padding: 0 10px;
    background: transparent;
    text-align: left;
}}
QPushButton[class="rail-row"]:hover {{ background: {surface_hover}; }}
QPushButton[class="rail-row"]:focus {{ background: {surface_hover}; }}
QPushButton[class="rail-row"]:checked {{ background: {accent_soft}; }}

QToolButton {{
    min-width: 30px;
    min-height: 30px;
    max-height: 30px;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 0 5px;
    background: transparent;
    color: {muted};
}}
QToolButton:focus {{ border-color: {accent}; }}
QToolButton:hover {{ color: {ink}; background: {surface_hover}; }}
QToolButton:checked {{ color: {accent}; background: {accent_soft}; }}

/* The search/clear buttons Qt embeds in a QLineEdit are QToolButtons too, and the rule
   above sizes them for the toolbar: Qt lays them out assuming the stock 18px side widget
   and the 30px minimum then overflows downward, dropping the glyph below the field's
   centre line. Give them back their natural size. */
QLineEdit QToolButton {{
    min-width: 0px;
    min-height: 0px;
    max-height: 16777215px;
    border: 0;
    padding: 0px;
    margin: 0px;
    background: transparent;
}}
QLineEdit QToolButton:hover, QLineEdit QToolButton:focus {{
    border: 0;
    background: transparent;
    color: {ink};
}}

/* ---------- containers ---------- */
QFrame[class="card"], QWidget[class="card"] {{
    border: 1px solid {line};
    border-radius: 14px;
    background: {surface};
}}
QFrame[class="panel"], QWidget[class="panel"] {{
    border: 1px solid {line};
    border-radius: 10px;
    background: {surface_soft};
}}
/* Painted as a filled 1px band rather than a border: a border on a box the same height
   as the border has nothing left to draw into, and the rule rendered as nothing. */
QFrame[class="hline"] {{ border: 0; background: {line}; min-height: 1px; max-height: 1px; }}
QFrame[class="vline"] {{ border: 0; border-left: 1px solid {line}; max-width: 1px; }}

#Sidebar {{
    background: {sidebar};
    border-right: 1px solid {line};
}}
#Topbar {{
    background: {bg};
    border-bottom: 1px solid {line};
}}
#ContextRail {{
    background: {sidebar};
    border-left: 1px solid {line};
}}

/* ---------- inputs ---------- */
QLineEdit, QSpinBox, QDoubleSpinBox {{
    min-height: 30px;
    max-height: 30px;
    border: 1px solid {line_strong};
    border-radius: 10px;
    padding: 0 11px;
    color: {text};
    background: {surface};
    /* The tinted selection every other list and popup in the app already uses. A solid
       accent block over a two-digit number read as an error state, not as a selection. */
    selection-background-color: {accent_soft};
    selection-color: {accent_ink};
}}
QTextEdit, QPlainTextEdit {{
    min-height: 32px;
    border: 1px solid {line_strong};
    border-radius: 10px;
    padding: 6px 11px;
    color: {text};
    background: {surface};
    selection-background-color: {accent_soft};
    selection-color: {accent_ink};
}}
QLineEdit:focus, QTextEdit:focus, QPlainTextEdit:focus,
QSpinBox:focus, QDoubleSpinBox:focus {{
    border-color: {accent};
}}
QLineEdit:disabled, QTextEdit:disabled, QPlainTextEdit:disabled {{
    color: {faint};
    background: {surface_soft};
}}
QComboBox {{
    min-height: 30px;
    max-height: 30px;
    border: 1px solid {line_strong};
    border-radius: 10px;
    padding: 0 11px;
    color: {text};
    background: {surface};
}}
QComboBox:hover {{ border-color: {line_strong}; }}
QComboBox:focus {{ border-color: {accent}; }}
QComboBox::drop-down {{ border: 0; width: 22px; }}
QComboBox QAbstractItemView {{
    border: 1px solid {line};
    border-radius: 10px;
    padding: 4px;
    color: {text};
    background: {surface};
    selection-background-color: {accent_soft};
    selection-color: {accent_ink};
    outline: none;
}}
QCheckBox, QRadioButton {{ color: {text}; spacing: 8px; }}
QCheckBox::indicator, QRadioButton::indicator {{
    width: 16px;
    height: 16px;
    border: 1px solid {line_strong};
    margin: 0;
    background: {surface};
}}
QCheckBox::indicator {{ border-radius: 4px; }}
QRadioButton::indicator {{ border-radius: 8px; }}
QCheckBox::indicator:checked, QRadioButton::indicator:checked {{
    border-color: {accent};
    background: {accent};
}}
QSlider::groove:horizontal {{ height: 4px; border-radius: 2px; background: {line}; }}
QSlider::handle:horizontal {{
    width: 14px;
    margin: -6px 0;
    border-radius: 7px;
    background: {accent};
}}

/* ---------- lists, tables, trees ---------- */
QListView, QTreeView, QTableView {{
    border: 1px solid {line};
    border-radius: 12px;
    color: {text};
    background: {surface};
    alternate-background-color: {surface_soft};
    selection-background-color: {accent_soft};
    selection-color: {accent_ink};
    outline: none;
}}
QListView[class="flat"], QTreeView[class="flat"], QTableView[class="flat"] {{
    border: 0;
    background: transparent;
}}
QListView::item, QTreeView::item, QTableView::item {{
    min-height: 28px;
    padding: 6px 8px;
    border-radius: 8px;
}}
QListView::item:hover, QTreeView::item:hover, QTableView::item:hover {{
    background: {surface_hover};
}}
QHeaderView::section {{
    padding: 8px 10px;
    border: 0;
    border-bottom: 1px solid {line};
    color: {muted};
    background: {surface_soft};
    font-size: {fxs}px;
    font-weight: 700;
}}
QTableView {{ gridline-color: {line}; }}
QTableCornerButton::section {{ background: {surface_soft}; border: 0; }}

/* ---------- tabs ---------- */
/* Document mode draws no pane, so the strip has to carry its own edge; a border box round
   the whole content would only add a second frame inside the page's own margins. */
QTabWidget::pane {{ border: 0; border-top: 1px solid {line}; background: transparent; }}
QTabWidget::tab-bar {{ left: 0; }}
/* The bar's own backdrop, stated. Left to the native style it painted a document-mode base
   from the *system* appearance, which put a black band across a light theme on a Mac set
   to dark. Nothing here may fall through to a palette the app does not control. */
QTabBar {{ background: transparent; border: 0; }}
QTabBar::tab {{
    min-height: 30px;
    margin-right: 2px;
    padding: 5px 14px;
    border: 0;
    border-radius: 8px;
    color: {muted};
    background: transparent;
    font-weight: 620;
}}
QTabBar::tab:hover {{ color: {ink}; background: {surface_hover}; }}
QTabBar::tab:selected {{ color: {accent_ink}; background: {accent_soft}; }}
QTabBar::tab:focus {{ color: {ink}; }}

/* ---------- scroll ---------- */
QScrollArea {{ border: 0; background: transparent; }}
QScrollBar:vertical {{ width: 10px; margin: 2px; background: transparent; }}
QScrollBar:horizontal {{ height: 10px; margin: 2px; background: transparent; }}
QScrollBar::handle:vertical, QScrollBar::handle:horizontal {{
    border-radius: 5px;
    background: {line_strong};
    min-height: 30px;
    min-width: 30px;
}}
QScrollBar::handle:hover {{ background: {muted}; }}
QScrollBar::add-line, QScrollBar::sub-line {{ height: 0; width: 0; }}
QScrollBar::add-page, QScrollBar::sub-page {{ background: transparent; }}

/* ---------- progress ---------- */
QProgressBar {{
    height: 6px;
    border: 0;
    border-radius: 3px;
    background: {line};
    text-align: center;
    color: {muted};
    font-size: {f2xs}px;
}}
QProgressBar::chunk {{ border-radius: 3px; background: {accent}; }}
QProgressBar[class="danger"]::chunk {{ background: {danger}; }}

/* ---------- menus ---------- */
QMenu {{
    border: 1px solid {line};
    border-radius: 12px;
    padding: 6px;
    color: {text};
    background: {surface};
}}
QMenu::item {{ min-height: 26px; border-radius: 8px; padding: 7px 14px; }}
QMenu::item:selected {{ color: {accent_ink}; background: {accent_soft}; }}
QMenu::separator {{ height: 1px; margin: 5px 8px; background: {line}; }}

QSplitter::handle {{ background: {line}; }}
QSplitter::handle:horizontal {{ width: 1px; }}
QSplitter::handle:vertical {{ height: 1px; }}

QTextBrowser {{ border: 0; background: transparent; color: {text}; }}
QGraphicsView {{ border: 0; background: {bg}; }}

{subcontrols}
"""
