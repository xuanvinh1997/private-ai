"""Lucide icons as inline SVG, rendered to ``QIcon`` in the current theme's ink colour.

The web app pulled these from ``lucide-solid`` and let CSS ``currentColor`` tint them.
Qt has no equivalent, so the stroke colour is substituted into the SVG source before
rasterising and the result is cached per (name, colour, size). ``apply_theme`` drops the
cache, which is why an icon never has to be told the theme changed.

Paths are the stock lucide 24x24 outline set, copied verbatim; only the wrapper is ours.
"""

from __future__ import annotations

import logging
from functools import lru_cache
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from PySide6.QtGui import QIcon, QPixmap

logger = logging.getLogger("private_ai.ui.icons")

__all__ = ["ICON_NAMES", "icon", "icon_path", "invalidate_cache", "pixmap", "svg_source"]

_MISSING_WARNED: set[str] = set()

# Body of each 24x24 lucide glyph. The stroke attributes live in the wrapper below.
PATHS: dict[str, str] = {
    "alert-triangle": (
        '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 '
        '1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>'
    ),
    "bell": (
        '<path d="M10.268 21a2 2 0 0 0 3.464 0"/>'
        '<path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673'
        'C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"/>'
    ),
    "book": (
        '<path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 '
        '1 0 0 1 0-5H20"/>'
    ),
    "book-open": (
        '<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 '
        "4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 "
        '0-3-3z"/>'
    ),
    "boxes": (
        '<path d="M2.97 12.92A2 2 0 0 0 2 14.63v3.24a2 2 0 0 0 .97 1.71l3 1.8a2 2 0 0 0 '
        '2.06 0L12 19v-5.5l-5-3-4.03 2.42Z"/><path d="m7 16.5-4.74-2.85"/>'
        '<path d="m7 16.5 5-3"/><path d="M7 16.5v5.17"/>'
        '<path d="M12 13.5V19l3.97 2.38a2 2 0 0 0 2.06 0l3-1.8a2 2 0 0 0 .97-1.71v-3.24a2 '
        '2 0 0 0-.97-1.71L17 10.5l-5 3Z"/><path d="m17 16.5-5-3"/>'
        '<path d="m17 16.5 4.74-2.85"/><path d="M17 16.5v5.17"/>'
        '<path d="M7.97 4.42A2 2 0 0 0 7 6.13v4.37l5 3 5-3V6.13a2 2 0 0 0-.97-1.71l-3-1.8a2 '
        '2 0 0 0-2.06 0l-3 1.8Z"/><path d="M12 8 7.26 5.15"/><path d="m12 8 4.74-2.85"/>'
        '<path d="M12 13.5V8"/>'
    ),
    "brain": (
        '<path d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 '
        '0 1 0 12 18Z"/><path d="M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 '
        '1-.556 6.588A4 4 0 1 1 12 18Z"/><path d="M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 '
        '1-3 4"/>'
    ),
    "check": '<path d="M20 6 9 17l-5-5"/>',
    "chevron-down": '<path d="m6 9 6 6 6-6"/>',
    "chevron-left": '<path d="m15 18-6-6 6-6"/>',
    "chevron-right": '<path d="m9 18 6-6-6-6"/>',
    "chevron-up": '<path d="m18 15-6-6-6 6"/>',
    "chevrons-up-down": '<path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/>',
    "copy": (
        '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/>'
        '<path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>'
    ),
    "cpu": (
        '<path d="M12 20v2"/><path d="M12 2v2"/><path d="M17 20v2"/><path d="M17 2v2"/>'
        '<path d="M2 12h2"/><path d="M2 17h2"/><path d="M2 7h2"/><path d="M20 12h2"/>'
        '<path d="M20 17h2"/><path d="M20 7h2"/><path d="M7 20v2"/><path d="M7 2v2"/>'
        '<rect x="4" y="4" width="16" height="16" rx="2"/>'
        '<rect x="8" y="8" width="8" height="8" rx="1"/>'
    ),
    "database": (
        '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/>'
        '<path d="M3 12A9 3 0 0 0 21 12"/>'
    ),
    "download": (
        '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>'
        '<polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>'
    ),
    "external-link": (
        '<path d="M15 3h6v6"/><path d="M10 14 21 3"/>'
        '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>'
    ),
    "eye": (
        '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 '
        '.696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>'
    ),
    "file-text": (
        '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/>'
        '<path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/>'
        '<path d="M16 17H8"/>'
    ),
    "filter": '<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>',
    "folder": (
        '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 '
        '0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>'
    ),
    "globe": (
        '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 '
        '0 0 0-20"/><path d="M2 12h20"/>'
    ),
    "hard-drive": (
        '<line x1="22" x2="2" y1="12" y2="12"/>'
        '<path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 '
        '0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><line x1="6" x2="6.01" y1="16" y2="16"/>'
        '<line x1="10" x2="10.01" y1="16" y2="16"/>'
    ),
    "info": ('<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>'),
    "layout-grid": (
        '<rect width="7" height="7" x="3" y="3" rx="1"/>'
        '<rect width="7" height="7" x="14" y="3" rx="1"/>'
        '<rect width="7" height="7" x="14" y="14" rx="1"/>'
        '<rect width="7" height="7" x="3" y="14" rx="1"/>'
    ),
    "loader": (
        '<path d="M12 2v4"/><path d="m16.2 7.8 2.9-2.9"/><path d="M18 12h4"/>'
        '<path d="m16.2 16.2 2.9 2.9"/><path d="M12 18v4"/><path d="m4.9 19.1 2.9-2.9"/>'
        '<path d="M2 12h4"/><path d="m4.9 4.9 2.9 2.9"/>'
    ),
    "menu": (
        '<line x1="4" x2="20" y1="12" y2="12"/><line x1="4" x2="20" y1="6" y2="6"/>'
        '<line x1="4" x2="20" y1="18" y2="18"/>'
    ),
    "message-square": ('<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>'),
    "message-square-text": (
        '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>'
        '<path d="M13 8H7"/><path d="M17 12H7"/>'
    ),
    "mic": (
        '<path d="M12 19v3"/>'
        '<path d="M19 10v2a7 7 0 0 1-14 0v-2"/>'
        '<rect x="9" y="2" width="6" height="13" rx="3"/>'
    ),
    "mic-off": (
        '<line x1="2" x2="22" y1="2" y2="22"/><path d="M18.89 13.23A7.12 7.12 0 0 0 19 '
        '12v-2"/><path d="M5 10v2a7 7 0 0 0 12 5"/><path d="M15 9.34V5a3 3 0 0 '
        '0-5.68-1.33"/><path d="M9 9v3a3 3 0 0 0 5.12 2.12"/><path d="M12 19v3"/>'
    ),
    "panel-left-close": (
        '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>'
        '<path d="m16 15-3-3 3-3"/>'
    ),
    "panel-left-open": (
        '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>'
        '<path d="m14 9 3 3-3 3"/>'
    ),
    "panel-right-close": (
        '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>'
        '<path d="m8 9 3 3-3 3"/>'
    ),
    "panel-right-open": (
        '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>'
        '<path d="m10 15-3-3 3-3"/>'
    ),
    "paperclip": (
        '<path d="M13.234 20.252 21 12.3a3.652 3.652 0 0 0-5.164-5.163L6.774 '
        "16.2a2.435 2.435 0 0 0 3.442 3.442l8.298-8.297a1.218 1.218 0 0 0-1.72-1.72l-7.44 "
        '7.44"/>'
    ),
    "pencil": (
        '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 '
        '4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>'
        '<path d="m15 5 4 4"/>'
    ),
    "play": '<polygon points="6 3 20 12 6 21 6 3"/>',
    "plus": '<path d="M5 12h14"/><path d="M12 5v14"/>',
    "refresh-cw": (
        '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>'
        '<path d="M21 3v5h-5"/>'
        '<path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>'
        '<path d="M8 16H3v5"/>'
    ),
    "search": '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
    "settings": (
        '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 '
        "1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 "
        "1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l"
        ".15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 "
        "2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-"
        ".39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 "
        "2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 "
        '0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>'
    ),
    "settings-2": (
        '<path d="M14 17H5"/><path d="M19 7h-9"/><circle cx="17" cy="17" r="3"/>'
        '<circle cx="7" cy="7" r="3"/>'
    ),
    "share-2": (
        '<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/>'
        '<circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/>'
        '<line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>'
    ),
    "sliders": (
        '<line x1="4" x2="4" y1="21" y2="14"/><line x1="4" x2="4" y1="10" y2="3"/>'
        '<line x1="12" x2="12" y1="21" y2="12"/><line x1="12" x2="12" y1="8" y2="3"/>'
        '<line x1="20" x2="20" y1="21" y2="16"/><line x1="20" x2="20" y1="12" y2="3"/>'
        '<line x1="2" x2="6" y1="14" y2="14"/><line x1="10" x2="14" y1="8" y2="8"/>'
        '<line x1="18" x2="22" y1="16" y2="16"/>'
    ),
    "sparkles": (
        '<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 '
        "0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 "
        "8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 "
        '0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/>'
    ),
    "stop-circle": (
        '<circle cx="12" cy="12" r="10"/><rect x="9" y="9" width="6" height="6" rx="1"/>'
    ),
    "trash-2": (
        '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/>'
        '<path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>'
        '<line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>'
    ),
    "upload": (
        '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>'
        '<polyline points="17 8 12 3 7 8"/><line x1="12" x2="12" y1="3" y2="15"/>'
    ),
    "user-plus": (
        '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>'
        '<circle cx="9" cy="7" r="4"/><line x1="19" x2="19" y1="8" y2="14"/>'
        '<line x1="22" x2="16" y1="11" y2="11"/>'
    ),
    "waypoints": (
        '<circle cx="12" cy="4.5" r="2.5"/><path d="m10.2 6.3-3.9 3.9"/>'
        '<circle cx="4.5" cy="12" r="2.5"/><path d="M7 12h10"/>'
        '<circle cx="19.5" cy="12" r="2.5"/><path d="m13.8 17.7 3.9-3.9"/>'
        '<circle cx="12" cy="19.5" r="2.5"/>'
    ),
    "wrench": (
        '<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 '
        "1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 "
        '3.76z"/>'
    ),
    "x": '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
    "zap": (
        '<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 '
        "0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 "
        '11 14z"/>'
    ),
}

ICON_NAMES: tuple[str, ...] = tuple(sorted(PATHS))

_WRAPPER = (
    '<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" '
    'viewBox="0 0 24 24" fill="none" stroke="{color}" stroke-width="1.8" '
    'stroke-linecap="round" stroke-linejoin="round">{body}</svg>'
)


def svg_source(name: str, color: str, size: int = 24) -> str:
    body = PATHS.get(name, "")
    return _WRAPPER.format(size=size, color=color, body=body)


def _ink() -> str:
    from private_ai.ui import theme

    return theme.token("ink")


@lru_cache(maxsize=512)
def _render(name: str, color: str, size: int) -> QPixmap:
    from PySide6.QtCore import QByteArray, Qt
    from PySide6.QtGui import QImage, QPainter, QPixmap
    from PySide6.QtSvg import QSvgRenderer

    # Rendered at 2x and scaled down: Qt's SVG rasteriser has no hinting, and a 20px
    # stroke drawn directly looks noticeably muddier than a downsampled 40px one.
    scale = 2
    renderer = QSvgRenderer(QByteArray(svg_source(name, color, 24).encode("utf-8")))
    image = QImage(size * scale, size * scale, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(Qt.GlobalColor.transparent)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    renderer.render(painter)
    painter.end()
    pix = QPixmap.fromImage(image)
    pix.setDevicePixelRatio(float(scale))
    return pix


def pixmap(name: str, size: int = 20, color: str | None = None) -> QPixmap:
    from PySide6.QtGui import QPixmap

    if name not in PATHS:
        _warn_missing(name)
        return QPixmap()
    return _render(name, color or _ink(), size)


def icon(name: str, *, color: str | None = None, size: int = 20) -> QIcon:
    """A themed icon. An unknown name yields an empty icon rather than an exception —
    a missing glyph must never take a screen down."""
    from PySide6.QtGui import QIcon

    if name not in PATHS:
        _warn_missing(name)
        return QIcon()
    return QIcon(_render(name, color or _ink(), size))


def _rasterise(name: str, color: str, pixels: int):
    """The glyph at an exact pixel size, as a ``QImage`` that can be written to disk."""
    from PySide6.QtCore import QByteArray, Qt
    from PySide6.QtGui import QImage, QPainter
    from PySide6.QtSvg import QSvgRenderer

    renderer = QSvgRenderer(QByteArray(svg_source(name, color, 24).encode("utf-8")))
    image = QImage(pixels, pixels, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(Qt.GlobalColor.transparent)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    renderer.render(painter)
    painter.end()
    return image


def _icon_cache_dir():
    from pathlib import Path

    from PySide6.QtCore import QStandardPaths

    root = QStandardPaths.writableLocation(QStandardPaths.StandardLocation.CacheLocation)
    if not root:
        import tempfile

        root = tempfile.gettempdir()
    folder = Path(root) / "qss-icons"
    folder.mkdir(parents=True, exist_ok=True)
    return folder


@lru_cache(maxsize=64)
def icon_path(name: str, color: str, size: int = 12) -> str:
    """A themed glyph written to disk, for the one consumer that cannot take a ``QPixmap``.

    Qt draws stylesheet sub-controls — a combo box's chevron, a spin box's steppers, a
    check box's tick — from ``image: url(...)`` and nothing else. Left unset it falls back
    to the *native* style for that one part, which is how a light theme ended up with
    system-drawn arrows sitting on top of its own rounded borders.

    The file name carries the colour, so switching theme writes new files rather than
    overwriting the ones the outgoing sheet still points at. A ``@2x`` twin is written
    beside each one, which is the convention Qt itself looks for on a retina screen.
    """
    from PySide6.QtGui import QGuiApplication

    if name not in PATHS or QGuiApplication.instance() is None:
        return ""
    try:
        folder = _icon_cache_dir()
        stem = f"{name}-{color.lstrip('#')}-{size}"
        target = folder / f"{stem}.png"
        if not target.exists():
            # Rasterised here rather than through ``_render``: that one hands back a shared,
            # cached pixmap carrying a 2x device ratio, and a file needs exact pixels.
            for suffix, pixels in ((".png", size), ("@2x.png", size * 2)):
                _rasterise(name, color, pixels).save(str(folder / f"{stem}{suffix}"), "PNG")
        return target.as_posix()
    except Exception:  # noqa: BLE001 - a missing arrow must not take the theme down
        logger.debug("Không ghi được biểu tượng cho stylesheet: %s", name, exc_info=True)
        return ""


def _warn_missing(name: str) -> None:
    if name not in _MISSING_WARNED:
        _MISSING_WARNED.add(name)
        logger.warning("Biểu tượng không tồn tại: %s", name)


def invalidate_cache() -> None:
    """Called by ``theme.apply_theme``: the ink colour is baked into every pixmap."""
    _render.cache_clear()
