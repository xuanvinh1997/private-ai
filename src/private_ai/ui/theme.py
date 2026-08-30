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
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from PySide6.QtWidgets import QApplication, QWidget

logger = logging.getLogger("private_ai.ui.theme")

__all__ = [
    "DARK",
    "LIGHT",
    "TYPE_SCALE",
    "apply_theme",
    "build_qss",
    "current_theme",
    "font_scale_px",
    "resolve_theme_name",
    "restyle",
    "token",
    "tokens",
    "type_scale",
]

# The 19 CSS custom properties, verbatim, plus five the web app expressed with
# color-mix() and Qt cannot compute at runtime.
LIGHT: dict[str, str] = {
    "bg": "#f3f6f4",
    "sidebar": "#edf2ef",
    "surface": "#ffffff",
    "surface-soft": "#f7f9f8",
    "surface-hover": "#e6ede9",
    "ink": "#17231f",
    "text": "#293732",
    "muted": "#62716c",
    "faint": "#87958f",
    "line": "#d8e0dc",
    "line-strong": "#c2cec8",
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
    "faint": "#71847c",
    "line": "#294039",
    "line-strong": "#365149",
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

# The CSS ladder in rem. Multiplied by the root size to get px, so "large text" is one
# number change and nothing lands below ~11px.
TYPE_SCALE: dict[str, float] = {
    "2xs": 0.74,
    "xs": 0.80,
    "sm": 0.86,
    "base": 0.92,
    "md": 1.00,
    "lg": 1.10,
    "xl": 1.30,
    "2xl": 1.60,
    "display": 2.00,
}

ROOT_PX: dict[str, int] = {"compact": 14, "normal": 15, "large": 18}

UI_FONTS = '"Manrope", "Manrope Variable", "Segoe UI", "Helvetica Neue", sans-serif'
MONO_FONTS = '"IBM Plex Mono", "SF Mono", "Cascadia Mono", "Consolas", monospace'

_current: dict[str, str] = {"theme": "light", "font_scale": "normal"}


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


def font_scale_px(scale: str) -> int:
    return ROOT_PX.get(scale, ROOT_PX["normal"])


def type_scale(scale: str = "") -> dict[str, int]:
    """The rem ladder resolved to whole pixels for the given root size."""
    root = font_scale_px(scale or _current["font_scale"])
    return {key: max(11, round(root * ratio)) for key, ratio in TYPE_SCALE.items()}


def restyle(widget: QWidget) -> None:
    """Re-evaluate stylesheet rules after a dynamic property changed.

    Qt caches the computed style per widget; changing ``class`` after the widget is shown
    does nothing until the style is unpolished and polished again.
    """
    style = widget.style()
    style.unpolish(widget)
    style.polish(widget)
    widget.update()


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
        ui=UI_FONTS,
        mono=MONO_FONTS,
        root=root,
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

    font = QFont()
    font.setPixelSize(font_scale_px(scale))
    # No bundled font files: Qt walks this list and lands on the platform default when
    # neither is installed, which is exactly what the CSS fallback chain did.
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
    font-size: {root}px;
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
QLabel[class="title"] {{ color: {ink}; font-size: {fxl}px; font-weight: 700; }}
QLabel[class="subtitle"] {{ color: {muted}; font-size: {fsm}px; }}
QLabel[class="display"] {{ color: {ink}; font-size: {fdisplay}px; font-weight: 760; }}
QLabel[class="heading"] {{ color: {ink}; font-size: {flg}px; font-weight: 700; }}
QLabel[class="muted"] {{ color: {muted}; font-size: {fsm}px; }}
QLabel[class="faint"] {{ color: {faint}; font-size: {fxs}px; }}
QLabel[class="danger"] {{ color: {danger}; font-size: {fsm}px; }}
QLabel[class="empty"] {{ color: {faint}; font-size: {fsm}px; padding: 18px 8px; }}
QLabel[class="section-label"] {{
    color: {muted};
    font-size: {f2xs}px;
    font-weight: 720;
    letter-spacing: 1px;
    padding: 0 7px;
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
    min-height: 32px;
    border: 1px solid {line};
    border-radius: 9px;
    padding: 6px 14px;
    color: {text};
    background: {surface};
    font-weight: 600;
}}
QPushButton:hover {{ background: {surface_hover}; border-color: {line_strong}; }}
QPushButton:pressed {{ background: {surface_hover}; }}
QPushButton:disabled {{ color: {faint}; background: {surface_soft}; border-color: {line}; }}
QPushButton:focus {{ border-color: {accent}; }}

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
QPushButton[class="danger"] {{
    color: {danger};
    background: {danger_soft};
    border-color: {danger_soft};
    font-weight: 700;
}}
QPushButton[class="danger"]:hover {{ border-color: {danger}; }}
QPushButton[class="ghost"] {{
    border: 0;
    background: transparent;
    color: {muted};
    font-weight: 580;
}}
QPushButton[class="ghost"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="icon"] {{
    min-width: 34px;
    max-width: 34px;
    min-height: 34px;
    max-height: 34px;
    border: 0;
    border-radius: 8px;
    padding: 0;
    background: transparent;
    color: {muted};
}}
QPushButton[class="icon"]:hover {{ color: {ink}; background: {surface_hover}; }}
QPushButton[class="icon"]:checked {{ color: {accent}; background: {accent_soft}; }}
QPushButton[class="chip"] {{
    min-height: 28px;
    border-radius: 999px;
    padding: 3px 13px;
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
QPushButton[class="nav-item"] {{
    min-height: 44px;
    border: 0;
    border-radius: 9px;
    padding: 0 13px;
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

QToolButton {{
    border: 0;
    border-radius: 8px;
    padding: 5px;
    background: transparent;
    color: {muted};
}}
QToolButton:hover {{ color: {ink}; background: {surface_hover}; }}
QToolButton:checked {{ color: {accent}; background: {accent_soft}; }}

/* ---------- containers ---------- */
QFrame[class="card"], QWidget[class="card"] {{
    border: 1px solid {line};
    border-radius: 14px;
    background: {surface};
}}
QFrame[class="panel"], QWidget[class="panel"] {{
    border: 1px solid {line};
    border-radius: 12px;
    background: {surface_soft};
}}
QFrame[class="hline"] {{ border: 0; border-top: 1px solid {line}; max-height: 1px; }}
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
QLineEdit, QTextEdit, QPlainTextEdit, QSpinBox, QDoubleSpinBox {{
    min-height: 30px;
    border: 1px solid {line};
    border-radius: 10px;
    padding: 6px 11px;
    color: {text};
    background: {surface};
    selection-background-color: {accent};
    selection-color: {on_accent};
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
    border: 1px solid {line};
    border-radius: 10px;
    padding: 4px 11px;
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
    padding: 6px 8px;
    border-radius: 8px;
}}
QListView::item:hover, QTreeView::item:hover, QTableView::item:hover {{
    background: {surface_hover};
}}
QHeaderView::section {{
    padding: 7px 9px;
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
QTabWidget::pane {{ border: 1px solid {line}; border-radius: 12px; background: {surface}; }}
QTabBar::tab {{
    margin-right: 4px;
    padding: 8px 15px;
    border: 1px solid transparent;
    border-radius: 9px;
    color: {muted};
    background: transparent;
    font-weight: 620;
}}
QTabBar::tab:hover {{ color: {ink}; background: {surface_hover}; }}
QTabBar::tab:selected {{ color: {accent_ink}; background: {accent_soft}; }}

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
QMenu::item {{ border-radius: 8px; padding: 7px 14px; }}
QMenu::item:selected {{ color: {accent_ink}; background: {accent_soft}; }}
QMenu::separator {{ height: 1px; margin: 5px 8px; background: {line}; }}

QSplitter::handle {{ background: {line}; }}
QSplitter::handle:horizontal {{ width: 1px; }}
QSplitter::handle:vertical {{ height: 1px; }}

QTextBrowser {{ border: 0; background: transparent; color: {text}; }}
QGraphicsView {{ border: 0; background: {bg}; }}
"""
