import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/** Token guard: an unresolved `var()` makes CSS drop the whole declaration silently, with no warning. */

const SRC = new URL("..", import.meta.url).pathname;

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return walk(path);
    return /\.(tsx?|css)$/.test(entry) ? [path] : [];
  });
}

function definedTokens(): Set<string> {
  const defined = new Set<string>();
  for (const file of ["styles/tokens.css", "styles/app.css"]) {
    const text = readFileSync(join(SRC, file), "utf8");
    for (const match of text.matchAll(/(--[a-z0-9-]+)\s*:/g)) defined.add(match[1]!);
  }
  return defined;
}

function usedTokens(): Map<string, string[]> {
  const used = new Map<string, string[]>();
  const note = (token: string, file: string) => {
    const at = used.get(token) ?? [];
    at.push(file.slice(SRC.length));
    used.set(token, at);
  };
  for (const file of walk(SRC)) {
    if (file.endsWith(".test.ts")) continue;
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(/var\((--[a-z0-9-]+)/g)) note(match[1]!, file);
    // Tailwind v4 shorthand: `w-(--foo)`, `px-(--foo)`.
    for (const match of text.matchAll(/[a-z]-\((--[a-z0-9-]+)\)/g)) note(match[1]!, file);
  }
  return used;
}

describe("khung viewport", () => {
  it("khóa cuộn tài liệu để vùng cuộn con không hở mép cửa sổ", () => {
    const css = readFileSync(join(SRC, "styles/app.css"), "utf8");
    const shell = css.match(/html,\s*body,\s*#root\s*\{([^}]*)\}/s)?.[1] ?? "";

    expect(shell).toMatch(/overflow\s*:\s*hidden\s*;/);
    expect(shell).toMatch(/overscroll-behavior\s*:\s*none\s*;/);
  });
});

describe("token CSS", () => {
  it("mọi biến được dùng đều có chỗ khai", () => {
    const defined = definedTokens();
    const missing = [...usedTokens().entries()]
      .filter(([token]) => !defined.has(token))
      .map(([token, files]) => `${token} (dùng ở ${files.join(", ")})`);

    expect(missing, "biến chưa khai làm CSS bỏ im lặng cả khai báo dùng nó").toEqual([]);
  });

  it("có đủ token bố cục mà khung ứng dụng dựa vào", () => {
    const defined = definedTokens();
    for (const token of [
      "--sidebar-w",
      "--workspace-panel-w",
      "--reading-measure",
      "--header-h",
    ]) {
      expect(defined.has(token), `thiếu ${token}`).toBe(true);
    }
  });

  it("ba cấp elevation đều có bóng ngoài thay vì chỉ có inset highlight", () => {
    const css = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
    const light = css.slice(css.indexOf(":root {"), css.indexOf(':root[data-theme="dark"]'));

    for (const token of ["--elevation-control", "--elevation-float", "--elevation-pop"]) {
      const value = light.match(new RegExp(`${token}\\s*:\\s*([^;]+);`))?.[1] ?? "";
      expect(value, `thiếu ${token}`).not.toBe("");
      expect(value, `${token} đang chỉ có inset nên UI vẫn bẹt`).toMatch(/,\s*0\s+\d/);
    }
  });

  it("thang z-index tăng đúng theo thứ tự lớp của ứng dụng", () => {
    const css = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
    const names = ["sticky", "floating", "screen", "popover", "modal", "toast", "tooltip"];
    const values = names.map((name) => {
      const value = css.match(new RegExp(`--z-${name}\\s*:\\s*(\\d+)`))?.[1];
      expect(value, `thiếu --z-${name}`).toBeDefined();
      return Number(value);
    });

    expect(values).toEqual([...values].sort((a, b) => a - b));
    expect(new Set(values).size).toBe(values.length);
  });

  // Every colour must be declared on bare `:root` first; declaring it only in the dark block breaks light mode.
  it("token của khối tối đều đã có bản sáng", () => {
    const text = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
    // Anchor on the selector `:root[data-theme="dark"]`; the bare string also occurs in the header comment.
    const darkAt = text.indexOf(':root[data-theme="dark"]');
    const rootBlock = text.slice(text.indexOf(":root {"), darkAt);
    const light = new Set([...rootBlock.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]!));

    const darkBlock = text.slice(darkAt);
    const dark = [...darkBlock.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]!);

    expect(dark.filter((token) => !light.has(token))).toEqual([]);
  });

  it("chữ và đường biên điều khiển đủ tương phản ở cả hai theme", () => {
    const css = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
    const darkAt = css.indexOf(':root[data-theme="dark"]');
    const mediaAt = css.indexOf("@media (prefers-color-scheme: dark)");
    const parse = (block: string) =>
      new Map(
        [...block.matchAll(/(--[a-z0-9-]+)\s*:\s*(#[0-9a-f]{6})\s*;/gi)].map((match) => [
          match[1]!,
          match[2]!,
        ]),
      );
    const themes = {
      light: parse(css.slice(css.indexOf(":root {"), darkAt)),
      dark: parse(css.slice(darkAt, mediaAt)),
    };
    const luminance = (hex: string) => {
      const channels = hex
        .slice(1)
        .match(/../g)!
        .map((channel) => Number.parseInt(channel, 16) / 255)
        .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
      return 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!;
    };
    const contrast = (foreground: string, background: string) => {
      const a = luminance(foreground);
      const b = luminance(background);
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    };

    for (const [theme, tokens] of Object.entries(themes)) {
      for (const background of ["--bg", "--surface"]) {
        for (const foreground of ["--ink", "--text", "--muted", "--faint"]) {
          expect(
            contrast(tokens.get(foreground)!, tokens.get(background)!),
            `${theme}: ${foreground} trên ${background}`,
          ).toBeGreaterThanOrEqual(4.5);
        }
        expect(
          contrast(tokens.get("--line-strong")!, tokens.get(background)!),
          `${theme}: biên điều khiển trên ${background}`,
        ).toBeGreaterThanOrEqual(3);
      }

      for (const [foreground, background] of [
        ["--on-accent", "--accent"],
        ["--accent-ink", "--accent-soft"],
        ["--warn", "--warn-soft"],
        ["--danger", "--danger-soft"],
      ] as const) {
        expect(
          contrast(tokens.get(foreground)!, tokens.get(background)!),
          `${theme}: ${foreground} trên ${background}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});

/** Type family: one system stack, nothing bundled. A woff2 that goes missing, or an `@import` the CSP blocks,
 * fails silently to a fallback - so the guard is that we never depend on a file or a host in the first place. */
describe("họ chữ hệ thống", () => {
  const css = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
  const stack = (token: string) =>
    css.match(new RegExp(`${token}\\s*:\\s*([^;]+);`))?.[1]!.replace(/\s+/g, " ").trim() ?? "";

  // `-apple-system` is the only name that resolves to the real SF Pro in WKWebView, which is the macOS runtime.
  it("--font-ui và --font-display bắt đầu bằng -apple-system", () => {
    for (const token of ["--font-ui", "--font-display"]) {
      expect(stack(token), `${token} chưa khai`).not.toBe("");
      expect(stack(token).startsWith("-apple-system"), `${token}: ${stack(token)}`).toBe(true);
    }
  });

  // SF Mono is not reachable through `-apple-system`; `ui-monospace` is how WebKit hands it over.
  it("--font-mono bắt đầu bằng ui-monospace", () => {
    expect(stack("--font-mono").startsWith("ui-monospace"), stack("--font-mono")).toBe(true);
  });

  // Every stack ends at a generic family, or a machine without any of the named ones picks its own default.
  it("mọi stack kết thúc bằng một họ chữ chung", () => {
    for (const token of ["--font-ui", "--font-display", "--font-mono"]) {
      expect(stack(token)).toMatch(/(sans-serif|serif|monospace)$/);
    }
  });

  // Nothing bundled and nothing remote: no `@font-face` to break, no host the CSP `default-src 'self'` blocks.
  it("không có @font-face và không tải font từ mạng", () => {
    for (const file of ["styles/tokens.css", "styles/app.css"]) {
      // Comments stripped first: both files explain in prose why there is no `@font-face` and no remote host.
      const text = readFileSync(join(SRC, file), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
      expect(text, `${file} còn @font-face`).not.toMatch(/@font-face/);
      expect(text, `${file} tải font từ mạng`).not.toMatch(/https?:/);
    }
    expect(existsSync(join(SRC, "assets/fonts")), "assets/fonts vẫn còn").toBe(false);
  });
});

/** Logo: the same mark is drawn in three files, and only one of them can read a token. The other two spell the
 * accent out by hand, which is exactly how they drifted to a green from a retired palette while the UI went coral. */
describe("màu logo", () => {
  const tokens = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
  const token = (name: string) =>
    tokens.match(new RegExp(`\\n\\s*${name}\\s*:\\s*(#[0-9a-f]{6})\\s*;`))?.[1]!;

  it("favicon trong index.html dùng đúng --accent", () => {
    const html = readFileSync(join(SRC, "../index.html"), "utf8");
    // The path lives in a data URI, so `#` is percent-encoded; compare on the six hex digits.
    expect(html, `favicon lệch --accent (${token("--accent")})`).toContain(
      `fill='%23${token("--accent")!.slice(1)}'`,
    );
  });

  it("icon hệ điều hành dùng đúng --accent và --accent-ink của chủ đề sáng", () => {
    const svg = readFileSync(join(SRC, "../../app/icons/icon-source.svg"), "utf8");
    for (const name of ["--accent", "--accent-ink"]) {
      expect(svg, `icon-source.svg thiếu ${name} (${token(name)})`).toContain(
        `stop-color="${token(name)}"`,
      );
    }
  });
});

/** Type scale: guards intent, not numbers - the smallest step must stay readable and the scale monotonic. */
describe("thang cỡ chữ", () => {
  const ORDER = ["2xs", "xs", "sm", "base", "md", "lg", "xl", "2xl", "display"];

  function scale(): Map<string, number> {
    const text = readFileSync(join(SRC, "styles/app.css"), "utf8");
    const sizes = new Map<string, number>();
    for (const name of ORDER) {
      const match = text.match(new RegExp(`^\\s*--text-${name}\\s*:\\s*([0-9.]+)rem;`, "m"));
      expect(match, `thiếu --text-${name}`).not.toBeNull();
      sizes.set(name, Number(match![1]));
    }
    return sizes;
  }

  it("bậc nhỏ nhất không xuống dưới 12px", () => {
    // Root is 15px (tokens.css), so 0.8rem = 12px; below that, filenames and diff line counts need squinting.
    expect(scale().get("2xs")!).toBeGreaterThanOrEqual(0.8);
  });

  it("thang đơn điệu tăng và base không vượt 1rem", () => {
    const sizes = scale();
    const values = ORDER.map((name) => sizes.get(name)!);
    expect(values).toEqual([...values].sort((a, b) => a - b));
    expect(new Set(values).size, "hai bậc trùng cỡ là một bậc thừa").toBe(values.length);
    // `--text-base` is body's default size; cap it at 1rem for density, :root[data-scale="large"] covers the rest.
    expect(sizes.get("base")!).toBeLessThanOrEqual(1);
  });
});
