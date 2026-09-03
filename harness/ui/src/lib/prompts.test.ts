import { describe, expect, it } from "vitest";

import { goiY } from "./prompts";
import type { PromptSeeds } from "./protocol";

const RONG: PromptSeeds = { symbols: [], directories: [], documents: [] };

const seeds = (part: Partial<PromptSeeds>): PromptSeeds => ({ ...RONG, ...part });

describe("goiY — không có nguyên liệu", () => {
  // Luật quan trọng nhất của cả tệp: màn hình trống không bao giờ được trống.
  it("lùi về bộ tĩnh cho cả ba loại dự án", () => {
    for (const kind of ["code", "docs", null] as const) {
      const ra = goiY(kind, RONG);
      expect(ra.length).toBeGreaterThanOrEqual(4);
      expect(ra.length).toBeLessThanOrEqual(5);
      expect(ra.every((cau) => cau.trim() !== "")).toBe(true);
    }
  });

  it("chưa mở dự án thì bỏ qua nguyên liệu kể cả khi có", () => {
    const co_du_lieu = seeds({ symbols: ["Harness"], documents: ["Hợp đồng"] });
    expect(goiY(null, co_du_lieu)).toEqual(goiY(null, RONG));
  });
});

describe("goiY — dự án mã nguồn", () => {
  it("dựng câu từ ký hiệu và thư mục có thật", () => {
    const ra = goiY("code", seeds({ symbols: ["CodeIndex", "Harness"], directories: ["crates/pai-rag"] }));
    expect(ra.slice(0, 3)).toEqual([
      "`CodeIndex` làm gì?",
      "Ai gọi `Harness`?",
      "Có gì trong `crates/pai-rag`?",
    ]);
  });

  it("bộ tĩnh lấp phần đuôi, tổng vẫn là năm", () => {
    const ra = goiY("code", seeds({ symbols: ["CodeIndex"] }));
    expect(ra).toHaveLength(5);
    expect(ra[0]).toBe("`CodeIndex` làm gì?");
    expect(ra[1]).toBe("Giải thích kiến trúc của dự án này");
  });

  it("không lấy tài liệu cho dự án mã nguồn", () => {
    expect(goiY("code", seeds({ documents: ["Hợp đồng thuê nhà"] }))).toEqual(goiY("code", RONG));
  });
});

describe("goiY — dự án tài liệu", () => {
  it("dựng câu từ tên tài liệu có thật", () => {
    const ra = goiY("docs", seeds({ documents: ["Hợp đồng thuê nhà", "Quy trình khôi phục"] }));
    expect(ra.slice(0, 2)).toEqual([
      "Tóm tắt “Hợp đồng thuê nhà” trong một câu",
      "“Hợp đồng thuê nhà” và “Quy trình khôi phục” khác nhau chỗ nào?",
    ]);
  });

  // Mời so sánh trong một thư viện một tệp là một gợi ý tự mâu thuẫn.
  it("một tài liệu thì không mời so sánh", () => {
    const ra = goiY("docs", seeds({ documents: ["Hợp đồng thuê nhà"] }));
    expect(ra.filter((cau) => cau.includes("khác nhau chỗ nào"))).toHaveLength(0);
  });

  it("không lấy ký hiệu cho dự án tài liệu", () => {
    expect(goiY("docs", seeds({ symbols: ["CodeIndex"] }))).toEqual(goiY("docs", RONG));
  });
});

describe("goiY — bất biến chung", () => {
  it("không bao giờ quá năm câu", () => {
    const day = seeds({
      symbols: ["A", "B", "C"],
      directories: ["x", "y", "z"],
      documents: ["1", "2", "3"],
    });
    expect(goiY("code", day)).toHaveLength(5);
    expect(goiY("docs", day)).toHaveLength(5);
  });

  it("không trùng câu", () => {
    const ra = goiY("code", seeds({ symbols: ["A", "B"], directories: ["x"] }));
    expect(new Set(ra).size).toBe(ra.length);
  });
});
