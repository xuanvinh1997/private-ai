import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Canh bảng token.
 *
 * Bài này sinh ra từ một lỗi thật: `ChangesPanel` đặt bề rộng bằng `w-(--changes-col-w)`
 * còn `tokens.css` chưa bao giờ khai biến ấy. CSS **bỏ im lặng** cả khai báo `width` khi
 * `var()` không phân giải được — không cảnh báo, không lỗi biên dịch, không dòng nào trong
 * console. Bảng chỉ đơn giản rộng theo nội dung, và không ai truy ra nguyên nhân vì chỗ
 * hỏng nằm ở một tệp không được nhắc tới.
 *
 * Đây đúng là loại lỗi mà con người không bắt được bằng mắt và máy bắt được trong một
 * phần nghìn giây.
 */

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
    // Lối viết rút gọn của Tailwind v4: `w-(--foo)`, `px-(--foo)`.
    for (const match of text.matchAll(/[a-z]-\((--[a-z0-9-]+)\)/g)) note(match[1]!, file);
  }
  return used;
}

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
    for (const token of ["--sidebar-w", "--changes-col-w", "--reading-measure", "--header-h"]) {
      expect(defined.has(token), `thiếu ${token}`).toBe(true);
    }
  });

  // Mọi màu phải khai ở `:root` trần trước, rồi mới ghi đè trong khối tối. Khai lần đầu
  // bên trong `[data-theme="dark"]` thì ở chế độ sáng nó không tồn tại, và thứ hỏng là màu
  // chữ trên nền sáng — đúng lỗi không thấy được nếu người sửa đang để máy ở chế độ tối.
  it("token của khối tối đều đã có bản sáng", () => {
    const text = readFileSync(join(SRC, "styles/tokens.css"), "utf8");
    // Mốc là **bộ chọn** `:root[data-theme="dark"]`, không phải chuỗi `[data-theme="dark"]`:
    // chuỗi ấy cũng nằm trong khối chú thích ở đầu tệp, tức là trước cả `:root`, và cắt
    // theo nó cho ra một lát ngược — bài kiểm chứng khi ấy hỏng vì chính nó, không vì CSS.
    const darkAt = text.indexOf(':root[data-theme="dark"]');
    const rootBlock = text.slice(text.indexOf(":root {"), darkAt);
    const light = new Set([...rootBlock.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]!));

    const darkBlock = text.slice(darkAt);
    const dark = [...darkBlock.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]!);

    expect(dark.filter((token) => !light.has(token))).toEqual([]);
  });
});

/**
 * Font đóng gói.
 *
 * Cùng một họ lỗi với bài ở trên: một `src: url(...)` trỏ vào tệp không tồn tại **không**
 * gây lỗi nào — trình duyệt bỏ qua khai báo `@font-face` ấy rồi lặng lẽ dùng font kế tiếp
 * trong stack. Trên máy người viết, người ấy thường đã cài sẵn Manrope nên chẳng thấy gì
 * khác; hỏng chỉ lộ ra trên máy người dùng, và lộ ra dưới dạng "app trông hơi lạ" chứ
 * không dưới dạng một dòng lỗi ai đó có thể tìm.
 */
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

  // App này viết bằng tiếng Việt. Thiếu subset `vietnamese` thì riêng ký tự có dấu rơi sang
  // font hệ thống, và một câu hiện ra bằng hai họ chữ trộn lẫn — dễ thấy nhưng khó gọi tên,
  // nên phải có bài kiểm gọi tên hộ.
  it("mỗi họ chữ đều có lát vietnamese", () => {
    // U+1EA0-1EF9 là khối chữ Việt có dấu; nó là dấu nhận biết chắc chắn nhất của lát này.
    const withVietnamese = new Set(
      faces
        .filter((face) => field(face, "unicode-range").includes("U+1EA0-1EF9"))
        .map((face) => field(face, "font-family")),
    );
    for (const family of ['"Manrope"', '"IBM Plex Mono"']) {
      expect(withVietnamese.has(family), `${family} thiếu subset vietnamese`).toBe(true);
    }
  });

  it("mọi @font-face đều dùng swap và khai unicode-range", () => {
    for (const face of faces) {
      expect(field(face, "font-display"), `thiếu font-display: ${face}`).toBe("swap");
      expect(field(face, "unicode-range"), `thiếu unicode-range: ${face}`).not.toBe("");
    }
  });

  // Không được để lọt một `@import`/`url()` tới máy chủ ngoài: CSP của app
  // (harness/app/tauri.conf.json) chỉ cho `default-src 'self'`, nên font ngoài không tải
  // được — và cũng không báo gì.
  it("không có font nào lấy từ mạng", () => {
    expect(css).not.toMatch(/https?:/);
  });
});

/**
 * Thang cỡ chữ.
 *
 * Bài này giữ *chủ đích*, không giữ con số: bậc nhỏ nhất phải còn đọc được vì `text-2xs`
 * là cỡ của toàn bộ metadata, và cả thang phải đơn điệu tăng vì một thang lộn xộn thì
 * `text-sm` có thể to hơn `text-base` mà không ai nhận ra.
 */
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
    // Root là 15px (tokens.css), nên 0.8rem = 12px. Dưới ngưỡng đó, đọc một tên tệp hay
    // một số dòng diff là phải nheo mắt — mà đó lại đúng là thứ người dùng cần đọc để
    // biết trợ lý vừa làm gì.
    expect(scale().get("2xs")!).toBeGreaterThanOrEqual(0.8);
  });

  it("thang đơn điệu tăng và base không vượt 1rem", () => {
    const sizes = scale();
    const values = ORDER.map((name) => sizes.get(name)!);
    expect(values).toEqual([...values].sort((a, b) => a - b));
    expect(new Set(values).size, "hai bậc trùng cỡ là một bậc thừa").toBe(values.length);
    // `--text-base` là cỡ mặc định của `body`; đẩy nó lên 1rem là đổi cả bố cục chứ không
    // chỉ đổi cỡ chữ, và người muốn to hơn đã có :root[data-scale="large"].
    expect(sizes.get("base")!).toBeLessThanOrEqual(1);
  });
});
