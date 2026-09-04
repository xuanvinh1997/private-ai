import { describe, expect, it } from "vitest";

import { S, t } from "./i18n";
import { goiY } from "./prompts";
import type { PromptSeeds } from "./protocol";

const RONG: PromptSeeds = { symbols: [], directories: [], documents: [] };

const seeds = (part: Partial<PromptSeeds>): PromptSeeds => ({ ...RONG, ...part });

describe("goiY — không có nguyên liệu", () => {
  // The most important rule here: the empty screen is never actually empty.
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
      t(S.libs.prompt.symbolWhat, { name: "CodeIndex" }),
      t(S.libs.prompt.symbolCallers, { name: "Harness" }),
      t(S.libs.prompt.dirContents, { path: "crates/pai-rag" }),
    ]);
  });

  it("bộ tĩnh lấp phần đuôi, tổng vẫn là năm", () => {
    const ra = goiY("code", seeds({ symbols: ["CodeIndex"] }));
    expect(ra).toHaveLength(5);
    expect(ra[0]).toBe(t(S.libs.prompt.symbolWhat, { name: "CodeIndex" }));
    expect(ra[1]).toBe(t(S.libs.prompt.codeArchitecture));
  });

  it("không lấy tài liệu cho dự án mã nguồn", () => {
    expect(goiY("code", seeds({ documents: ["Hợp đồng thuê nhà"] }))).toEqual(goiY("code", RONG));
  });
});

describe("goiY — dự án tài liệu", () => {
  it("dựng câu từ tên tài liệu có thật", () => {
    const ra = goiY("docs", seeds({ documents: ["Hợp đồng thuê nhà", "Quy trình khôi phục"] }));
    expect(ra.slice(0, 2)).toEqual([
      t(S.libs.prompt.docSummary, { title: "Hợp đồng thuê nhà" }),
      t(S.libs.prompt.docCompare, { first: "Hợp đồng thuê nhà", second: "Quy trình khôi phục" }),
    ]);
  });

  // Offering a comparison in a one-document library is a self-contradicting suggestion.
  it("một tài liệu thì không mời so sánh", () => {
    const ra = goiY("docs", seeds({ documents: ["Hợp đồng thuê nhà"] }));
    const soSanh = t(S.libs.prompt.docCompare, { first: "Hợp đồng thuê nhà", second: "x" });
    expect(ra).not.toContain(soSanh);
    expect(ra.filter((cau) => cau.includes("Hợp đồng thuê nhà"))).toHaveLength(1);
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
