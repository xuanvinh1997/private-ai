import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
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
