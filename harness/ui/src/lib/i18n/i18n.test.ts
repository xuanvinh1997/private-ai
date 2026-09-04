import { describe, expect, it } from "vitest";
import { S, type Msg } from "./index";
import { split } from "./rich";

/** Types catch a *missing* translation; what they cannot catch is two translations drifting apart in slots or marks. */

function walk(node: unknown, path: string, out: [string, Msg][]): void {
  if (node === null || typeof node !== "object") return;
  const record = node as Record<string, unknown>;
  if (typeof record.en === "string" && typeof record.vi === "string") {
    out.push([path, node as Msg]);
    return;
  }
  for (const [key, value] of Object.entries(record)) walk(value, `${path}.${key}`, out);
}

const MESSAGES: [string, Msg][] = [];
walk(S, "S", MESSAGES);

const slots = (raw: string): string[] =>
  [...raw.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort();

describe("catalog i18n", () => {
  it("có chuỗi để kiểm", () => {
    expect(MESSAGES.length).toBeGreaterThan(400);
  });

  it("không chuỗi nào rỗng", () => {
    for (const [path, msg] of MESSAGES) {
      expect(msg.en.trim(), path).not.toBe("");
      expect(msg.vi.trim(), path).not.toBe("");
    }
  });

  it("hai ngôn ngữ dùng cùng bộ chỗ trống", () => {
    for (const [path, msg] of MESSAGES) {
      expect(slots(msg.vi), path).toEqual(slots(msg.en));
    }
  });

  it("dấu nhấn đóng mở cân nhau", () => {
    for (const [path, msg] of MESSAGES) {
      for (const raw of [msg.en, msg.vi]) {
        expect((raw.match(/\*/g) ?? []).length % 2, `${path}: ${raw}`).toBe(0);
        expect((raw.match(/`/g) ?? []).length % 2, `${path}: ${raw}`).toBe(0);
      }
    }
  });
});

describe("split", () => {
  it("tách phần in đậm và phần chữ máy", () => {
    expect(split("đọc `~/.ssh` là *được*")).toEqual([
      { text: "đọc ", mark: null },
      { text: "~/.ssh", mark: "code" },
      { text: " là ", mark: null },
      { text: "được", mark: "b" },
    ]);
  });

  it("chuỗi không dấu trả về một mảnh", () => {
    expect(split("Gửi")).toEqual([{ text: "Gửi", mark: null }]);
  });

  it("dấu mở không có dấu đóng vẫn là chữ đọc được", () => {
    expect(split("thiếu *dấu đóng")).toEqual([{ text: "thiếu *dấu đóng", mark: null }]);
  });
});
