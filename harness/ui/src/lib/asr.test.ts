import { describe, expect, it } from "vitest";

import { elapsed, meterFill, micQuiet } from "./asr";
import type { DictationState } from "./asr";

const DANG_GHI: DictationState = {
  phase: "recording",
  committed: "",
  tentative: "",
  recordedMs: 0,
  openMs: 0,
  level: 0,
  heardMs: -1,
  device: "micro",
  streaming: true,
  error: null,
};

const ghi = (part: Partial<DictationState>): DictationState => ({ ...DANG_GHI, ...part });

describe("meterFill", () => {
  it("im lặng tuyệt đối thì thanh mức trống", () => {
    expect(meterFill(0)).toBe(0);
  });

  it("tiếng ồn nền dưới sàn vẫn là trống, không phải một vạch nhấp nháy", () => {
    expect(meterFill(0.002)).toBe(0);
  });

  it("giọng nói bình thường lấp phần lớn thanh mức", () => {
    expect(meterFill(0.3)).toBeGreaterThan(0.7);
  });

  it("càng to càng đầy, và không bao giờ vượt 1", () => {
    expect(meterFill(0.05)).toBeLessThan(meterFill(0.2));
    expect(meterFill(1)).toBe(1);
  });
});

describe("micQuiet", () => {
  it("không cảnh báo trong vài giây đầu: chưa đủ để kết luận gì", () => {
    expect(micQuiet(ghi({ openMs: 1_000 }))).toBe(false);
  });

  it("mở lâu mà chưa nghe thấy gì thì báo", () => {
    expect(micQuiet(ghi({ openMs: 5_000 }))).toBe(true);
  });

  it("khoảng nghỉ giữa hai câu không tính là im", () => {
    expect(micQuiet(ghi({ openMs: 10_000, heardMs: 8_500 }))).toBe(false);
  });

  it("từng nghe thấy nhưng đã im lâu thì báo: micro vừa bị tắt tiếng cũng trông như thế", () => {
    expect(micQuiet(ghi({ openMs: 10_000, heardMs: 2_000 }))).toBe(true);
  });

  it("lúc chưa ghi thì không có gì để nói", () => {
    expect(micQuiet(ghi({ phase: "loading", openMs: 10_000 }))).toBe(false);
  });
});

describe("elapsed", () => {
  it("đọc theo `m:ss`, giây luôn hai chữ số", () => {
    expect(elapsed(0)).toBe("0:00");
    expect(elapsed(7_400)).toBe("0:07");
    expect(elapsed(125_000)).toBe("2:05");
  });
});
