"""Interactive charts as a single self-contained HTML file.

No chart library. A page written here has to open from ``file://`` on a machine that has
never been online, which rules out a CDN, and vendoring a bundle to get a line chart
would put megabytes of third-party JavaScript in the repository for something a few
hundred lines of canvas code does exactly. What is here is what a reader actually uses:
hover readout, a legend that toggles series, wheel zoom and drag pan along the x axis.

The renderer below is written once as a Python string constant. The only thing that
varies between two generated pages is the JSON spec handed to it.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field

from private_ai.core.artifacts.page import escape, js_literal, render_page
from private_ai.core.artifacts.store import ArtifactError

__all__ = ["CHART_TYPES", "ChartSeries", "ChartSpec", "render_chart_page"]

CHART_TYPES = ("line", "area", "bar", "stacked_bar", "candlestick", "scatter", "pie")

VALUE_FORMATS = ("number", "currency", "percent", "compact")

# A chart nobody can read is not a chart. Above this the marks are narrower than the
# gaps between them and the tooltip is the only way to read a value, which means the
# picture has stopped carrying the information.
MAX_POINTS = 5000
MAX_SERIES = 12


@dataclass(frozen=True, slots=True)
class ChartSeries:
    """One line, one set of bars, or one pie's worth of slices."""

    name: str
    values: Sequence[float]


@dataclass(frozen=True, slots=True)
class ChartCandle:
    """One session of a price chart. ``volume`` is optional and drawn as a sub-panel."""

    label: str
    open: float
    high: float
    low: float
    close: float
    volume: float | None = None


@dataclass(frozen=True, slots=True)
class ChartSpec:
    title: str
    chart_type: str = "line"
    subtitle: str = ""
    categories: Sequence[str] = ()
    series: Sequence[ChartSeries] = ()
    candles: Sequence[ChartCandle] = ()
    x_label: str = ""
    y_label: str = ""
    unit: str = ""
    value_format: str = "number"
    decimals: int = -1
    source: str = ""
    notes: Sequence[str] = field(default=())

    def validate(self) -> None:
        """Refuse a chart that would render as a lie or as an empty box.

        Every message names the field and what it should have held: the caller is a model
        reading the tool's error text, and a vague failure just gets retried unchanged.
        """
        if not self.title.strip():
            raise ArtifactError("Thiếu 'title': biểu đồ cần một tiêu đề.")
        if self.chart_type not in CHART_TYPES:
            raise ArtifactError(
                f"chart_type '{self.chart_type}' không hỗ trợ. Chọn một trong: "
                + ", ".join(CHART_TYPES)
            )
        if self.value_format not in VALUE_FORMATS:
            raise ArtifactError(
                f"value_format '{self.value_format}' không hợp lệ. Chọn: "
                + ", ".join(VALUE_FORMATS)
            )
        if self.chart_type == "candlestick":
            self._validate_candles()
            return
        self._validate_series()

    def _validate_candles(self) -> None:
        if not self.candles:
            raise ArtifactError(
                "Biểu đồ nến cần 'candles': danh sách phiên có open/high/low/close."
            )
        if len(self.candles) > MAX_POINTS:
            raise ArtifactError(f"Quá nhiều phiên ({len(self.candles)}); tối đa {MAX_POINTS}.")
        for index, candle in enumerate(self.candles):
            low = min(candle.open, candle.close)
            high = max(candle.open, candle.close)
            if candle.high < high or candle.low > low:
                raise ArtifactError(
                    f"Phiên #{index + 1} ({candle.label}): high/low không bao được open/close. "
                    "Kiểm tra lại thứ tự bốn giá trị."
                )

    def _validate_series(self) -> None:
        if not self.series:
            raise ArtifactError("Thiếu 'series': cần ít nhất một chuỗi số liệu.")
        if len(self.series) > MAX_SERIES:
            raise ArtifactError(f"Quá nhiều chuỗi ({len(self.series)}); tối đa {MAX_SERIES}.")
        if self.chart_type == "pie" and len(self.series) != 1:
            raise ArtifactError("Biểu đồ tròn chỉ nhận đúng một chuỗi; mỗi giá trị là một phần.")
        width = len(self.categories)
        for entry in self.series:
            if not entry.values:
                raise ArtifactError(f"Chuỗi '{entry.name}' không có giá trị nào.")
            if len(entry.values) > MAX_POINTS:
                raise ArtifactError(
                    f"Chuỗi '{entry.name}' có {len(entry.values)} điểm; tối đa {MAX_POINTS}."
                )
            # A series shorter than its labels is the common failure and it silently
            # shifts every point left, so it is refused rather than padded.
            if width and len(entry.values) != width:
                raise ArtifactError(
                    f"Chuỗi '{entry.name}' có {len(entry.values)} giá trị nhưng có "
                    f"{width} nhãn trong 'categories'. Hai con số phải bằng nhau."
                )
        if not width and len({len(entry.values) for entry in self.series}) > 1:
            raise ArtifactError("Các chuỗi phải cùng độ dài khi không có 'categories'.")

    def payload(self) -> dict[str, object]:
        """The spec as the browser sees it. Keys are camelCase because JS reads them."""
        length = (
            len(self.candles)
            if self.chart_type == "candlestick"
            else max((len(entry.values) for entry in self.series), default=0)
        )
        categories = list(self.categories) or (
            [candle.label for candle in self.candles]
            if self.chart_type == "candlestick"
            else [str(index + 1) for index in range(length)]
        )
        return {
            "type": self.chart_type,
            "categories": categories,
            "series": [
                {"name": entry.name, "values": [float(value) for value in entry.values]}
                for entry in self.series
            ],
            "candles": [
                {
                    "label": candle.label,
                    "o": float(candle.open),
                    "h": float(candle.high),
                    "l": float(candle.low),
                    "c": float(candle.close),
                    "v": None if candle.volume is None else float(candle.volume),
                }
                for candle in self.candles
            ],
            "xLabel": self.x_label,
            "yLabel": self.y_label,
            "unit": self.unit,
            "valueFormat": self.value_format,
            "decimals": self.decimals,
        }


_CHART_STYLE = """
.chart-shell { position: relative; }
canvas#chart {
  width: 100%; height: 440px; display: block; touch-action: none; cursor: crosshair;
}
#legend {
  display: flex; flex-wrap: wrap; gap: 8px; margin: 0 0 14px; padding: 0; list-style: none;
}
#legend button {
  display: inline-flex; align-items: center; gap: 7px;
  background: transparent; border: 1px solid var(--border); border-radius: 999px;
  padding: 4px 11px 4px 8px; font: inherit; font-size: 13px; color: var(--ink); cursor: pointer;
}
#legend button[aria-pressed="false"] { color: var(--muted); opacity: .55; }
#legend .swatch { width: 10px; height: 10px; border-radius: 3px; flex: 0 0 auto; }
#tip {
  position: absolute; pointer-events: none; opacity: 0; transition: opacity .08s linear;
  background: var(--surface); border: 1px solid var(--border); border-radius: 9px;
  box-shadow: var(--shadow); padding: 9px 11px; font-size: 13px; min-width: 130px;
  z-index: 2; color: var(--ink);
}
#tip .tip-head { font-weight: 600; margin-bottom: 5px; }
#tip .tip-row { display: flex; align-items: center; gap: 7px; white-space: nowrap; }
#tip .tip-row .swatch { width: 8px; height: 8px; border-radius: 2px; flex: 0 0 auto; }
#tip .tip-row .tip-name { color: var(--muted); }
#tip .tip-row .tip-value { margin-left: auto; font-variant-numeric: tabular-nums; }
.hint { color: var(--muted); font-size: 12.5px; margin: 12px 0 0; }
.notes { margin: 14px 0 0; padding-left: 20px; color: var(--muted); font-size: 13.5px; }
.notes li { margin: 3px 0; }
"""

# The whole renderer. It reads one global, ``SPEC``, which the page defines just above.
_CHART_SCRIPT = r"""
(function () {
  var spec = SPEC;
  var canvas = document.getElementById('chart');
  if (!canvas) return;
  var ctx = canvas.getContext('2d');
  var tip = document.getElementById('tip');
  var legendEl = document.getElementById('legend');

  var LIGHT = ['#2f6fd0','#e07b39','#17915f','#8e5cd6','#cf3f45','#0f8fa8','#b58900','#6b7785',
               '#4c6ef5','#d6336c','#2b8a3e','#e8590c'];
  var DARK  = ['#6fa8ff','#f5a25d','#35c48b','#b98cf0','#f0757b','#4fc3d9','#e0bf4a','#aab4c4',
               '#8ea0ff','#ff8fb1','#5fd08a','#ffab6b'];

  var isCandle = spec.type === 'candlestick';
  var isPie = spec.type === 'pie';
  var isBar = spec.type === 'bar' || spec.type === 'stacked_bar';
  var isStacked = spec.type === 'stacked_bar';
  var isScatter = spec.type === 'scatter';
  var isArea = spec.type === 'area';

  var N = isCandle ? spec.candles.length
                   : (spec.series.length ? spec.series[0].values.length : 0);
  var hasVolume = isCandle && spec.candles.some(function (c) {
    return c.v !== null && c.v !== undefined;
  });

  var hidden = {};
  var view = { a: 0, b: Math.max(0, N - 1) };
  var hover = -1;
  var W = 0, H = 0;
  var colors = LIGHT;
  var ink = '#16191d', muted = '#667085', grid = '#eceef2', surface = '#fff';
  var up = '#17915f', down = '#cf3f45';

  // --- theme -------------------------------------------------------------

  function isDark() {
    var explicit = document.documentElement.getAttribute('data-theme');
    if (explicit) return explicit === 'dark';
    return window.matchMedia('(prefers-color-scheme: dark)').matches;
  }
  function readTheme() {
    var style = getComputedStyle(document.documentElement);
    function v(name, fallback) { return (style.getPropertyValue(name) || '').trim() || fallback; }
    colors = isDark() ? DARK : LIGHT;
    ink = v('--ink', ink); muted = v('--muted', muted); grid = v('--grid', grid);
    surface = v('--surface', surface); up = v('--up', up); down = v('--down', down);
  }
  function colorAt(i) { return colors[i % colors.length]; }

  // --- numbers -----------------------------------------------------------

  function decimalsFor(value) {
    if (spec.decimals >= 0) return spec.decimals;
    var size = Math.abs(value);
    if (size === 0) return 0;
    if (size >= 1000) return 0;
    if (size >= 10) return 1;
    if (size >= 1) return 2;
    return 4;
  }
  function group(value, digits) {
    return new Intl.NumberFormat('vi-VN', {
      minimumFractionDigits: digits, maximumFractionDigits: digits
    }).format(value);
  }
  function compact(value) {
    return new Intl.NumberFormat('vi-VN', { notation: 'compact', maximumFractionDigits: 1 })
      .format(value);
  }
  function unitSuffix() { return spec.unit ? ' ' + spec.unit : ''; }
  function fmt(value) {
    if (value === null || value === undefined || !isFinite(value)) return '—';
    if (spec.valueFormat === 'compact') return compact(value) + unitSuffix();
    var text = group(value, decimalsFor(value));
    if (spec.valueFormat === 'percent') return text + '%';
    if (spec.valueFormat === 'currency') return text + (spec.unit ? ' ' + spec.unit : ' ₫');
    return text + unitSuffix();
  }
  // One decimal count for the whole axis, taken from the gap between ticks. Deciding
  // per value prints "0,00" next to "200" on the same axis, which reads as two scales.
  var axisDigits = 0;
  function setAxisDigits(ticks) {
    if (spec.decimals >= 0) { axisDigits = spec.decimals; return; }
    // Whole numbers stay whole: "16" and "14", not "16,0" and "14,0".
    if (ticks.every(function (t) { return Math.abs(t - Math.round(t)) < 1e-9; })) {
      axisDigits = 0;
      return;
    }
    var gap = ticks.length > 1 ? Math.abs(ticks[1] - ticks[0]) : Math.abs(ticks[0] || 1);
    axisDigits = gap >= 10 ? 0 : gap >= 1 ? 1 : gap >= 0.1 ? 2
      : Math.min(6, Math.ceil(-Math.log10(gap)) + 1);
  }
  function fmtAxis(value) {
    if (Math.abs(value) >= 10000) return compact(value);
    return group(value, axisDigits);
  }

  function niceTicks(min, max, wanted) {
    if (!isFinite(min) || !isFinite(max)) return [0];
    if (min === max) { min -= 1; max += 1; }
    var rough = (max - min) / Math.max(1, wanted);
    var magnitude = Math.pow(10, Math.floor(Math.log10(rough)));
    var scaled = rough / magnitude;
    var step = (scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 2.5 ? 2.5 : scaled <= 5 ? 5 : 10)
      * magnitude;
    var out = [];
    for (var value = Math.ceil(min / step) * step; value <= max + step * 1e-9; value += step) {
      out.push(Math.abs(value) < step * 1e-9 ? 0 : value);
    }
    return out.length ? out : [min, max];
  }

  // --- data views --------------------------------------------------------

  function activeSeries() {
    return spec.series
      .map(function (s, i) { return { name: s.name, values: s.values, index: i }; })
      .filter(function (s) { return !hidden[s.index]; });
  }
  function range() {
    return [Math.max(0, Math.floor(view.a)), Math.min(N - 1, Math.ceil(view.b))];
  }
  function valueDomain() {
    var bounds = range(), lo = Infinity, hi = -Infinity;
    if (isCandle) {
      for (var i = bounds[0]; i <= bounds[1]; i++) {
        var candle = spec.candles[i];
        if (!candle) continue;
        lo = Math.min(lo, candle.l); hi = Math.max(hi, candle.h);
      }
    } else if (isStacked) {
      var live = activeSeries();
      for (var j = bounds[0]; j <= bounds[1]; j++) {
        var positive = 0, negative = 0;
        live.forEach(function (s) {
          var value = s.values[j];
          if (!isFinite(value)) return;
          if (value >= 0) positive += value; else negative += value;
        });
        lo = Math.min(lo, negative); hi = Math.max(hi, positive);
      }
    } else {
      activeSeries().forEach(function (s) {
        for (var k = bounds[0]; k <= bounds[1]; k++) {
          var value = s.values[k];
          if (!isFinite(value)) continue;
          lo = Math.min(lo, value); hi = Math.max(hi, value);
        }
      });
    }
    if (!isFinite(lo) || !isFinite(hi)) return [0, 1];
    // Bars are read against zero, so a bar chart that does not include it exaggerates
    // every difference on it. Lines and candles are read against each other instead.
    if (isBar) { lo = Math.min(lo, 0); hi = Math.max(hi, 0); }
    if (lo === hi) { lo -= Math.abs(lo || 1) * 0.1; hi += Math.abs(hi || 1) * 0.1; }
    else { var pad = (hi - lo) * 0.08; lo -= pad; hi += pad; }
    if (isBar) { if (lo > 0) lo = 0; if (hi < 0) hi = 0; }
    return [lo, hi];
  }

  // --- layout ------------------------------------------------------------

  var plot = { x: 0, y: 0, w: 0, h: 0 };
  var volumePanel = { y: 0, h: 0 };
  var step = 1, domain = [0, 1];

  function measure() {
    domain = valueDomain();
    var ticks = niceTicks(domain[0], domain[1], 6);
    setAxisDigits(ticks);
    ctx.font = '12px system-ui, sans-serif';
    var widest = 0;
    ticks.forEach(function (t) { widest = Math.max(widest, ctx.measureText(fmtAxis(t)).width); });
    var left = isPie ? 8 : Math.ceil(widest) + 14 + (spec.yLabel ? 16 : 0);
    var bottom = isPie ? 8 : 34 + (spec.xLabel ? 16 : 0);
    plot = { x: left, y: 10, w: Math.max(10, W - left - 12), h: Math.max(10, H - 10 - bottom) };
    if (hasVolume) {
      volumePanel.h = plot.h * 0.22;
      volumePanel.y = plot.y + plot.h - volumePanel.h;
      plot.h -= volumePanel.h + 10;
    }
    var count = Math.max(1, view.b - view.a + 1);
    step = plot.w / count;
    return ticks;
  }
  // The bottom of the drawing, volume sub-panel included. Category labels hang off this
  // rather than off the price panel, which they would otherwise be drawn on top of.
  function baseline() {
    return hasVolume ? volumePanel.y + volumePanel.h : plot.y + plot.h;
  }
  function xAt(index) { return plot.x + (index - view.a + 0.5) * step; }
  function yAt(value) {
    var span = domain[1] - domain[0] || 1;
    return plot.y + plot.h - ((value - domain[0]) / span) * plot.h;
  }
  function indexAt(px) {
    var raw = Math.round((px - plot.x) / step - 0.5 + view.a);
    return Math.max(0, Math.min(N - 1, raw));
  }

  // --- drawing -----------------------------------------------------------

  function resize() {
    var ratio = window.devicePixelRatio || 1;
    var rect = canvas.getBoundingClientRect();
    W = rect.width; H = rect.height;
    canvas.width = Math.max(1, Math.round(W * ratio));
    canvas.height = Math.max(1, Math.round(H * ratio));
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  function draw() {
    readTheme();
    ctx.clearRect(0, 0, W, H);
    if (!N) {
      ctx.fillStyle = muted; ctx.font = '14px system-ui, sans-serif';
      ctx.textAlign = 'center'; ctx.fillText('Không có dữ liệu', W / 2, H / 2);
      return;
    }
    if (isPie) { drawPie(); return; }
    var ticks = measure();
    drawGrid(ticks);
    if (isCandle) { drawCandles(); if (hasVolume) drawVolume(); }
    else if (isBar) drawBars();
    else drawLines();
    drawCrosshair();
  }

  function drawGrid(ticks) {
    ctx.save();
    ctx.font = '12px system-ui, sans-serif';
    ctx.strokeStyle = grid; ctx.lineWidth = 1;
    ctx.fillStyle = muted; ctx.textAlign = 'right'; ctx.textBaseline = 'middle';
    ticks.forEach(function (value) {
      var y = Math.round(yAt(value)) + 0.5;
      if (y < plot.y - 1 || y > plot.y + plot.h + 1) return;
      ctx.beginPath(); ctx.moveTo(plot.x, y); ctx.lineTo(plot.x + plot.w, y); ctx.stroke();
      ctx.fillText(fmtAxis(value), plot.x - 8, y);
    });
    // The zero line is a different statement from a gridline: it is where sign flips.
    if (domain[0] < 0 && domain[1] > 0) {
      var zero = Math.round(yAt(0)) + 0.5;
      ctx.strokeStyle = muted; ctx.globalAlpha = 0.5;
      ctx.beginPath(); ctx.moveTo(plot.x, zero); ctx.lineTo(plot.x + plot.w, zero); ctx.stroke();
      ctx.globalAlpha = 1;
    }
    ctx.textAlign = 'center'; ctx.textBaseline = 'top';
    var bounds = range();
    var widest = 10;
    for (var i = bounds[0]; i <= bounds[1]; i++) {
      widest = Math.max(widest, ctx.measureText(String(spec.categories[i] || '')).width);
    }
    var stride = Math.max(1, Math.ceil((widest + 18) / step));
    var floor = baseline();
    for (var j = bounds[0]; j <= bounds[1]; j += stride) {
      var x = xAt(j);
      if (x < plot.x - 1 || x > plot.x + plot.w + 1) continue;
      ctx.fillText(String(spec.categories[j] || ''), x, floor + 8);
    }
    if (spec.xLabel) {
      ctx.fillStyle = muted;
      ctx.fillText(spec.xLabel, plot.x + plot.w / 2, floor + 26);
    }
    if (spec.yLabel) {
      ctx.save();
      ctx.translate(12, plot.y + plot.h / 2); ctx.rotate(-Math.PI / 2);
      ctx.textAlign = 'center'; ctx.textBaseline = 'top';
      ctx.fillStyle = muted; ctx.fillText(spec.yLabel, 0, 0);
      ctx.restore();
    }
    ctx.restore();
  }

  function drawLines() {
    var bounds = range();
    ctx.save();
    ctx.beginPath();
    ctx.rect(plot.x, plot.y - 4, plot.w, plot.h + 8);
    ctx.clip();
    activeSeries().forEach(function (s) {
      var color = colorAt(s.index);
      if (!isScatter) {
        ctx.beginPath();
        var open = false;
        for (var i = bounds[0]; i <= bounds[1]; i++) {
          var value = s.values[i];
          if (!isFinite(value)) { open = false; continue; }
          var x = xAt(i), y = yAt(value);
          if (!open) { ctx.moveTo(x, y); open = true; } else ctx.lineTo(x, y);
        }
        ctx.strokeStyle = color; ctx.lineWidth = 2;
        ctx.lineJoin = 'round'; ctx.lineCap = 'round';
        ctx.stroke();
        if (isArea) {
          ctx.lineTo(xAt(bounds[1]), yAt(Math.max(domain[0], 0)));
          ctx.lineTo(xAt(bounds[0]), yAt(Math.max(domain[0], 0)));
          ctx.closePath();
          ctx.globalAlpha = 0.16; ctx.fillStyle = color; ctx.fill(); ctx.globalAlpha = 1;
        }
      }
      // Markers only when they are far enough apart to be marks rather than a smear.
      if (isScatter || step > 14) {
        ctx.fillStyle = color;
        for (var j = bounds[0]; j <= bounds[1]; j++) {
          var point = s.values[j];
          if (!isFinite(point)) continue;
          ctx.beginPath();
          ctx.arc(xAt(j), yAt(point), isScatter ? 3.6 : 2.8, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    });
    ctx.restore();
  }

  function drawBars() {
    var bounds = range();
    var live = activeSeries();
    if (!live.length) return;
    var slot = step * 0.72;
    var width = isStacked ? slot : slot / live.length;
    ctx.save();
    ctx.beginPath(); ctx.rect(plot.x, plot.y - 4, plot.w, plot.h + 8); ctx.clip();
    for (var i = bounds[0]; i <= bounds[1]; i++) {
      var center = xAt(i);
      var risen = 0, fallen = 0;
      live.forEach(function (s, slotIndex) {
        var value = s.values[i];
        if (!isFinite(value)) return;
        var x = isStacked ? center - slot / 2 : center - slot / 2 + slotIndex * width;
        var top, bottom;
        if (isStacked) {
          var base = value >= 0 ? risen : fallen;
          top = yAt(base + value); bottom = yAt(base);
          if (value >= 0) risen += value; else fallen += value;
        } else {
          top = yAt(Math.max(value, 0)); bottom = yAt(Math.min(value, 0));
        }
        ctx.fillStyle = colorAt(s.index);
        ctx.globalAlpha = hover === i || hover < 0 ? 1 : 0.55;
        ctx.fillRect(x, Math.min(top, bottom), Math.max(1, width - 1), Math.abs(bottom - top));
      });
    }
    ctx.globalAlpha = 1;
    ctx.restore();
  }

  function drawCandles() {
    var bounds = range();
    var width = Math.max(1, Math.min(step * 0.66, 22));
    ctx.save();
    ctx.beginPath(); ctx.rect(plot.x, plot.y - 4, plot.w, plot.h + 8); ctx.clip();
    for (var i = bounds[0]; i <= bounds[1]; i++) {
      var candle = spec.candles[i];
      if (!candle) continue;
      var rising = candle.c >= candle.o;
      var color = rising ? up : down;
      var x = xAt(i);
      ctx.strokeStyle = color; ctx.fillStyle = color; ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(Math.round(x) + 0.5, yAt(candle.h));
      ctx.lineTo(Math.round(x) + 0.5, yAt(candle.l));
      ctx.stroke();
      var top = yAt(Math.max(candle.o, candle.c));
      var height = Math.max(1, Math.abs(yAt(candle.o) - yAt(candle.c)));
      if (width < 3) {
        ctx.fillRect(Math.round(x), top, 1, height);
      } else if (rising) {
        // Hollow up, filled down: the direction stays legible without relying on hue.
        ctx.fillStyle = surface;
        ctx.fillRect(x - width / 2, top, width, height);
        ctx.strokeRect(x - width / 2 + 0.5, top + 0.5, width - 1, Math.max(1, height - 1));
      } else {
        ctx.fillRect(x - width / 2, top, width, height);
      }
    }
    ctx.restore();
  }

  function drawVolume() {
    var bounds = range(), peak = 0;
    for (var i = bounds[0]; i <= bounds[1]; i++) {
      var candle = spec.candles[i];
      if (candle && isFinite(candle.v)) peak = Math.max(peak, candle.v);
    }
    if (!peak) return;
    var width = Math.max(1, Math.min(step * 0.66, 22));
    ctx.save();
    ctx.beginPath();
    ctx.rect(plot.x, volumePanel.y, plot.w, volumePanel.h);
    ctx.clip();
    ctx.globalAlpha = 0.45;
    for (var j = bounds[0]; j <= bounds[1]; j++) {
      var bar = spec.candles[j];
      if (!bar || !isFinite(bar.v)) continue;
      var height = (bar.v / peak) * volumePanel.h;
      ctx.fillStyle = bar.c >= bar.o ? up : down;
      ctx.fillRect(xAt(j) - width / 2, volumePanel.y + volumePanel.h - height, width, height);
    }
    ctx.restore();
    ctx.save();
    ctx.fillStyle = muted; ctx.font = '11px system-ui, sans-serif';
    ctx.textAlign = 'left'; ctx.textBaseline = 'top';
    ctx.fillText('Khối lượng', plot.x + 4, volumePanel.y + 2);
    ctx.restore();
  }

  function drawPie() {
    var values = (spec.series[0] || { values: [] }).values;
    var total = values.reduce(function (sum, v) {
      return sum + (isFinite(v) && v > 0 ? v : 0);
    }, 0);
    if (!total) return;
    var radius = Math.min(W, H) / 2 - 30;
    var cx = W / 2, cy = H / 2;
    var angle = -Math.PI / 2;
    ctx.save();
    ctx.font = '12px system-ui, sans-serif';
    values.forEach(function (value, i) {
      if (!isFinite(value) || value <= 0) return;
      var sweep = (value / total) * Math.PI * 2;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, radius, angle, angle + sweep);
      ctx.closePath();
      ctx.fillStyle = colorAt(i);
      ctx.globalAlpha = hover === i ? 1 : 0.9;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.strokeStyle = surface; ctx.lineWidth = 2; ctx.stroke();
      var share = value / total;
      if (share > 0.045) {
        var mid = angle + sweep / 2;
        ctx.fillStyle = '#fff';
        ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
        ctx.fillText(group(share * 100, share < 0.1 ? 1 : 0) + '%',
          cx + Math.cos(mid) * radius * 0.68, cy + Math.sin(mid) * radius * 0.68);
      }
      angle += sweep;
    });
    ctx.restore();
  }

  function drawCrosshair() {
    if (hover < 0 || hover < view.a - 0.5 || hover > view.b + 0.5) return;
    var x = xAt(hover);
    ctx.save();
    ctx.strokeStyle = muted; ctx.globalAlpha = 0.45;
    ctx.setLineDash([4, 4]); ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(Math.round(x) + 0.5, plot.y);
    ctx.lineTo(Math.round(x) + 0.5, baseline());
    ctx.stroke();
    ctx.restore();
  }

  // --- legend and tooltip ------------------------------------------------

  function buildLegend() {
    if (!legendEl) return;
    legendEl.innerHTML = '';
    var entries = isPie
      ? spec.categories.map(function (label, i) { return { label: label, index: i, pie: true }; })
      : spec.series.map(function (s, i) { return { label: s.name, index: i, pie: false }; });
    if (entries.length < 2 && !isPie) return;
    entries.forEach(function (entry) {
      var button = document.createElement('button');
      button.type = 'button';
      button.setAttribute('aria-pressed', hidden[entry.index] ? 'false' : 'true');
      var swatch = document.createElement('span');
      swatch.className = 'swatch';
      swatch.style.background = colorAt(entry.index);
      button.appendChild(swatch);
      button.appendChild(document.createTextNode(entry.label));
      if (!entry.pie) {
        button.addEventListener('click', function () {
          // Never let the last one be switched off: an empty plot reads as broken.
          if (!hidden[entry.index] && activeSeries().length <= 1) return;
          hidden[entry.index] = !hidden[entry.index];
          button.setAttribute('aria-pressed', hidden[entry.index] ? 'false' : 'true');
          draw();
        });
      } else {
        button.style.cursor = 'default';
      }
      legendEl.appendChild(button);
    });
  }

  function showTip(clientX, clientY) {
    if (!tip || hover < 0) { hideTip(); return; }
    var rows = [];
    var heading = String(spec.categories[hover] || '');
    if (isCandle) {
      var candle = spec.candles[hover];
      if (!candle) { hideTip(); return; }
      var change = candle.o ? ((candle.c - candle.o) / candle.o) * 100 : 0;
      rows = [
        ['Mở', fmt(candle.o)], ['Cao', fmt(candle.h)],
        ['Thấp', fmt(candle.l)], ['Đóng', fmt(candle.c)],
        ['Thay đổi', group(change, 2) + '%']
      ];
      if (isFinite(candle.v)) rows.push(['Khối lượng', compact(candle.v)]);
      tip.innerHTML = '<div class="tip-head">' + escapeHtml(heading) + '</div>' +
        rows.map(function (row) {
          return '<div class="tip-row"><span class="tip-name">' + row[0] +
            '</span><span class="tip-value">' + escapeHtml(row[1]) + '</span></div>';
        }).join('');
    } else if (isPie) {
      var values = (spec.series[0] || { values: [] }).values;
      var total = values.reduce(function (s, v) { return s + (isFinite(v) && v > 0 ? v : 0); }, 0);
      var value = values[hover];
      tip.innerHTML = '<div class="tip-head">' + escapeHtml(heading) + '</div>' +
        '<div class="tip-row"><span class="tip-value">' + escapeHtml(fmt(value)) +
        '</span></div><div class="tip-row"><span class="tip-name">Tỷ trọng</span>' +
        '<span class="tip-value">' + group(total ? (value / total) * 100 : 0, 1) + '%</span></div>';
    } else {
      var body = activeSeries().map(function (s) {
        return '<div class="tip-row"><span class="swatch" style="background:' + colorAt(s.index) +
          '"></span><span class="tip-name">' + escapeHtml(s.name) +
          '</span><span class="tip-value">' + escapeHtml(fmt(s.values[hover])) + '</span></div>';
      }).join('');
      tip.innerHTML = '<div class="tip-head">' + escapeHtml(heading) + '</div>' + body;
    }
    var shell = canvas.parentNode.getBoundingClientRect();
    var left = clientX - shell.left + 14;
    var top = clientY - shell.top + 14;
    tip.style.opacity = '1';
    if (left + tip.offsetWidth > shell.width) left = clientX - shell.left - tip.offsetWidth - 14;
    if (top + tip.offsetHeight > shell.height) top = shell.height - tip.offsetHeight - 4;
    tip.style.left = Math.max(0, left) + 'px';
    tip.style.top = Math.max(0, top) + 'px';
  }
  function hideTip() { if (tip) tip.style.opacity = '0'; }
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, function (ch) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch];
    });
  }

  // --- interaction -------------------------------------------------------

  var dragging = false, dragX = 0, dragAnchor = 0;

  canvas.addEventListener('pointermove', function (event) {
    var rect = canvas.getBoundingClientRect();
    if (dragging && !isPie) {
      var moved = (event.clientX - dragX) / step;
      var span = view.b - view.a;
      var start = Math.max(0, Math.min(N - 1 - span, dragAnchor - moved));
      view.a = start; view.b = start + span;
      draw();
      return;
    }
    if (isPie) { hover = pieIndexAt(event.clientX - rect.left, event.clientY - rect.top); }
    else if (event.clientY - rect.top > baseline() + 6) { hover = -1; }
    else { hover = indexAt(event.clientX - rect.left); }
    draw();
    if (hover >= 0) showTip(event.clientX, event.clientY); else hideTip();
  });

  canvas.addEventListener('pointerleave', function () { hover = -1; hideTip(); draw(); });

  canvas.addEventListener('pointerdown', function (event) {
    if (isPie || view.b - view.a >= N - 1) return;
    dragging = true; dragX = event.clientX; dragAnchor = view.a;
    canvas.setPointerCapture(event.pointerId);
    canvas.style.cursor = 'grabbing';
  });
  function endDrag(event) {
    if (!dragging) return;
    dragging = false;
    canvas.style.cursor = 'crosshair';
    if (event && event.pointerId !== undefined) {
      try { canvas.releasePointerCapture(event.pointerId); } catch (e) {}
    }
  }
  canvas.addEventListener('pointerup', endDrag);
  canvas.addEventListener('pointercancel', endDrag);

  canvas.addEventListener('wheel', function (event) {
    if (isPie || N < 4) return;
    event.preventDefault();
    var rect = canvas.getBoundingClientRect();
    var anchor = view.a + (event.clientX - rect.left - plot.x) / step;
    var span = view.b - view.a;
    var next = span * (event.deltaY > 0 ? 1.25 : 0.8);
    next = Math.max(2, Math.min(N - 1, next));
    var ratio = span > 0 ? (anchor - view.a) / span : 0.5;
    var start = anchor - ratio * next;
    start = Math.max(0, Math.min(N - 1 - next, start));
    view.a = start; view.b = start + next;
    draw();
  }, { passive: false });

  canvas.addEventListener('dblclick', function () {
    view = { a: 0, b: Math.max(0, N - 1) };
    draw();
  });

  function pieIndexAt(x, y) {
    var radius = Math.min(W, H) / 2 - 30;
    var cx = W / 2, cy = H / 2;
    var dx = x - cx, dy = y - cy;
    if (Math.sqrt(dx * dx + dy * dy) > radius) return -1;
    var values = (spec.series[0] || { values: [] }).values;
    var total = values.reduce(function (s, v) { return s + (isFinite(v) && v > 0 ? v : 0); }, 0);
    if (!total) return -1;
    var angle = Math.atan2(dy, dx) + Math.PI / 2;
    if (angle < 0) angle += Math.PI * 2;
    var walked = 0;
    for (var i = 0; i < values.length; i++) {
      var value = isFinite(values[i]) && values[i] > 0 ? values[i] : 0;
      walked += (value / total) * Math.PI * 2;
      if (angle <= walked) return i;
    }
    return -1;
  }

  var frame = null;
  function schedule() {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(function () { frame = null; resize(); draw(); });
  }
  window.addEventListener('resize', schedule);
  window.addEventListener('private-ai-theme', function () { buildLegend(); draw(); });
  window.matchMedia('(prefers-color-scheme: dark)')
    .addEventListener('change', function () { buildLegend(); draw(); });

  readTheme();
  buildLegend();
  resize();
  draw();
})();
"""


def render_chart_page(spec: ChartSpec) -> str:
    """One validated spec as a complete HTML document."""
    spec.validate()
    notes = ""
    if spec.notes:
        items = "".join(f"<li>{escape(note)}</li>" for note in spec.notes)
        notes = f'<ul class="notes">{items}</ul>'
    hint = (
        "Di chuột để đọc giá trị · cuộn để phóng to · kéo để trượt · nhấp đúp để đặt lại"
        if spec.chart_type != "pie"
        else "Di chuột để đọc giá trị từng phần"
    )
    body = (
        '<div class="card">'
        '<ul id="legend"></ul>'
        '<div class="chart-shell"><canvas id="chart"></canvas><div id="tip"></div></div>'
        f'<p class="hint">{hint}</p>'
        f"{notes}"
        "</div>"
    )
    script = f"var SPEC = {js_literal(spec.payload())};\n{_CHART_SCRIPT}"
    return render_page(
        title=spec.title,
        subtitle=spec.subtitle,
        body=body,
        style=_CHART_STYLE,
        script=script,
        source=spec.source,
    )
