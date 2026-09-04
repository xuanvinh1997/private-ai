import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
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

/** Bundled fonts: a `src: url(...)` pointing at a missing file is skipped silently, so only users ever see it. */
describe("font đóng gói", () => {
  const FONTS_CSS = join(SRC, "styles/fonts.css");
  const css = readFileSync(FONTS_CSS, "utf8");

  const faces = [...css.matchAll(/@font-face\s*\{([^}]*)\}/g)].map((m) => m[1]!);
  const field = (face: string, name: string) =>
    face.match(new RegExp(`${name}\\s*:\\s*([^;]+);`))?.[1]?.trim() ?? "";

  it("mọi url() trong fonts.css trỏ tới tệp có thật", () => {
    const urls = [...css.matchAll(/url\("([^"]+)"\)/g)].map((m) => m[1]!);
    expect(urls.length, "fonts.css không khai tệp font nào").toBeGreaterThan(0);

    const missing = urls.filter((u) => !existsSync(resolve(dirname(FONTS_CSS), u)));
    expect(missing, "url() hỏng làm @font-face bị bỏ im lặng").toEqual([]);
  });

  // The app is in Vietnamese: without the `vietnamese` subset, accented characters fall back to another family.
  it("mỗi họ chữ đều có lát vietnamese", () => {
    // U+1EA0-1EF9 is the Vietnamese accented block, the surest marker of this subset.
    const withVietnamese = new Set(
      faces
        .filter((face) => field(face, "unicode-range").includes("U+1EA0-1EF9"))
        .map((face) => field(face, "font-family")),
    );
    for (const family of ['"Manrope"', '"IBM Plex Mono"', '"EB Garamond"']) {
      expect(withVietnamese.has(family), `${family} thiếu subset vietnamese`).toBe(true);
    }
  });

  it("mọi @font-face đều dùng swap và khai unicode-range", () => {
    for (const face of faces) {
      expect(field(face, "font-display"), `thiếu font-display: ${face}`).toBe("swap");
      expect(field(face, "unicode-range"), `thiếu unicode-range: ${face}`).not.toBe("");
    }
  });

  // No `@import`/`url()` to an outside host: the app CSP is `default-src 'self'`, so remote fonts fail silently.
  it("không có font nào lấy từ mạng", () => {
    expect(css).not.toMatch(/https?:/);
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
