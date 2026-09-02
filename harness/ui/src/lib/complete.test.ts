import { describe, expect, it } from "vitest";

import { applyCompletion, findTrigger, rankCommands } from "./complete";

describe("findTrigger — lệnh", () => {
  it("mở khi `/` là ký tự đầu tiên", () => {
    expect(findTrigger("/mo", 3)).toEqual({ kind: "command", query: "mo", start: 0, end: 3 });
  });

  it("`/` trần mở danh sách đầy đủ", () => {
    expect(findTrigger("/", 1)?.query).toBe("");
  });

  // Luật quan trọng nhất của cả tệp: đường dẫn là thứ gõ suốt ngày trong ứng dụng này.
  it("KHÔNG mở khi `/` nằm giữa một đường dẫn", () => {
    expect(findTrigger("src/lib", 7)).toBeNull();
    expect(findTrigger("xem crates/pai-fs", 17)).toBeNull();
  });

  it("đóng lại khi đã gõ qua một khoảng trắng", () => {
    expect(findTrigger("/moi phien", 10)).toBeNull();
  });
});

describe("findTrigger — tệp", () => {
  it("mở ở đầu ô nhập", () => {
    expect(findTrigger("@sto", 4)).toEqual({ kind: "path", query: "sto", start: 0, end: 4 });
  });

  it("mở sau một khoảng trắng", () => {
    const trigger = findTrigger("đọc @store", 10);
    expect(trigger).toEqual({ kind: "path", query: "store", start: 4, end: 10 });
  });

  it("KHÔNG mở khi `@` nằm giữa từ", () => {
    expect(findTrigger("a@b", 3)).toBeNull();
    expect(findTrigger("mail@example.com", 16)).toBeNull();
  });

  it("đóng lại khi đã gõ qua một khoảng trắng", () => {
    expect(findTrigger("@store rồi", 10)).toBeNull();
  });

  it("bám `@` gần con trỏ nhất khi có nhiều dấu", () => {
    expect(findTrigger("@một @hai", 9)?.query).toBe("hai");
  });

  it("chỉ nhìn phần trước con trỏ", () => {
    // Con trỏ đứng sau `@st`; phần `ore` phía sau không thuộc truy vấn.
    expect(findTrigger("@store", 3)?.query).toBe("st");
  });

  it("không có dấu dẫn thì không có gì", () => {
    expect(findTrigger("chỉ là chữ", 10)).toBeNull();
  });
});

describe("applyCompletion", () => {
  it("thay phần đang gõ và thêm một dấu cách", () => {
    const trigger = findTrigger("đọc @sto", 8)!;
    const out = applyCompletion("đọc @sto", trigger, "src/store.rs");
    expect(out.text).toBe("đọc src/store.rs ");
    expect(out.caret).toBe(out.text.length);
  });

  it("giữ nguyên phần đứng sau con trỏ", () => {
    const trigger = findTrigger("@sto rồi gì đó", 4)!;
    const out = applyCompletion("@sto rồi gì đó", trigger, "a/b.rs");
    expect(out.text).toBe("a/b.rs  rồi gì đó");
  });
});

describe("rankCommands", () => {
  it("truy vấn rỗng trả về mọi lệnh", () => {
    expect(rankCommands("").length).toBeGreaterThan(5);
  });

  it("khớp tiền tố tên đứng trên khớp giữa tên", () => {
    expect(rankCommands("m")[0]?.name).toBe("mcp");
  });

  it("tìm được qua câu mô tả, không chỉ qua tên", () => {
    expect(rankCommands("phím tắt").map((c) => c.name)).toContain("phimtat");
  });

  it("không khớp thì rỗng", () => {
    expect(rankCommands("khongcolenhnay")).toEqual([]);
  });
});
