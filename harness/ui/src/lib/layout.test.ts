import { describe, expect, it } from "vitest";
import { clampPanelWidth, panelWidthFromDrag } from "./layout";

describe("clampPanelWidth", () => {
  it("giữ chiều rộng trong giới hạn và làm tròn pixel", () => {
    expect(clampPanelWidth(199, 220, 480)).toBe(220);
    expect(clampPanelWidth(321.6, 220, 480)).toBe(322);
    expect(clampPanelWidth(900, 220, 480)).toBe(480);
  });
});

describe("panelWidthFromDrag", () => {
  it("kéo mép trái của panel phải sang trái để nới rộng", () => {
    expect(panelWidthFromDrag(300, 200, 160, "left")).toBe(340);
    expect(panelWidthFromDrag(300, 200, 240, "left")).toBe(260);
  });

  it("kéo mép phải của sidebar trái sang phải để nới rộng", () => {
    expect(panelWidthFromDrag(268, 268, 308, "right")).toBe(308);
    expect(panelWidthFromDrag(268, 268, 228, "right")).toBe(228);
  });
});
