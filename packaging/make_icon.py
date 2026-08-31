"""Build ``PrivateAI.icns`` from the app's own brand mark.

The three ascending bars are the mark ``_BrandMark`` paints in the sidebar, at the same
proportions. Drawing the icon from the same geometry rather than commissioning a separate
one is the whole point: the thing in the Dock and the thing at the top of the rail are the
same mark, and neither can drift from the other.

macOS wants an ``.icns``, which is an ``.iconset`` directory run through ``iconutil``.
Run this directly, or let ``build.sh`` do it.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The rounded-square plate every macOS icon sits on, as a fraction of the canvas. Apple's
# grid leaves the outer ~10% clear and rounds at ~22% of the plate.
PLATE_INSET = 0.10
PLATE_RADIUS = 0.225

# The mark itself, as fractions of the plate: three bars of rising height and opacity.
BAR_COUNT = 3
BAR_WIDTH = 0.150
BAR_GAP = 0.085
# Heights plus the baseline offset below must stay under 1.0, or the tallest bar grows
# out through the top of the plate.
BAR_HEIGHTS = (0.34, 0.53, 0.72)
BAR_ALPHAS = (140, 199, 255)
BAR_RADIUS = 0.055

# The app's accent, and a plate a shade off pure white so the mark is not floating on the
# desktop when the wallpaper happens to be light.
ACCENT = "#2f6fd0"
PLATE_TOP = "#ffffff"
PLATE_BOTTOM = "#eef1f6"

# Every size iconutil expects, and the @2x variants that go with them.
ICON_SIZES = (16, 32, 128, 256, 512)


def _render(size: int):
    from PySide6.QtCore import QRectF, Qt
    from PySide6.QtGui import QColor, QImage, QLinearGradient, QPainter

    image = QImage(size, size, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(Qt.GlobalColor.transparent)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setPen(Qt.PenStyle.NoPen)

    inset = size * PLATE_INSET
    plate = QRectF(inset, inset, size - inset * 2, size - inset * 2)
    gradient = QLinearGradient(plate.topLeft(), plate.bottomLeft())
    gradient.setColorAt(0.0, QColor(PLATE_TOP))
    gradient.setColorAt(1.0, QColor(PLATE_BOTTOM))
    painter.setBrush(gradient)
    radius = plate.width() * PLATE_RADIUS
    painter.drawRoundedRect(plate, radius, radius)

    accent = QColor(ACCENT)
    span = BAR_COUNT * BAR_WIDTH + (BAR_COUNT - 1) * BAR_GAP
    left = plate.left() + (plate.width() - plate.width() * span) / 2
    # The bars stand on a common baseline inside the plate, not on the plate's own edge.
    baseline = plate.bottom() - plate.height() * 0.14
    for index in range(BAR_COUNT):
        color = QColor(accent)
        color.setAlpha(BAR_ALPHAS[index])
        painter.setBrush(color)
        height = plate.height() * BAR_HEIGHTS[index]
        bar = QRectF(
            left + index * plate.width() * (BAR_WIDTH + BAR_GAP),
            baseline - height,
            plate.width() * BAR_WIDTH,
            height,
        )
        corner = bar.width() * (BAR_RADIUS / BAR_WIDTH)
        painter.drawRoundedRect(bar, corner, corner)
    painter.end()
    return image


def build(output: Path) -> Path:
    from PySide6.QtGui import QGuiApplication

    # Offscreen: this runs in a build script, with no display and no window server.
    application = QGuiApplication.instance() or QGuiApplication(
        [sys.argv[0], "-platform", "offscreen"]
    )
    iconset = output.with_suffix(".iconset")
    iconset.mkdir(parents=True, exist_ok=True)
    for size in ICON_SIZES:
        _render(size).save(str(iconset / f"icon_{size}x{size}.png"))
        _render(size * 2).save(str(iconset / f"icon_{size}x{size}@2x.png"))
    subprocess.run(  # noqa: S603
        ["/usr/bin/iconutil", "--convert", "icns", "--output", str(output), str(iconset)],
        check=True,
    )
    del application
    return output


if __name__ == "__main__":
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "PrivateAI.icns"
    print(build(target))
