export function clampPanelWidth(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function panelWidthFromDrag(
  startWidth: number,
  startX: number,
  currentX: number,
  edge: "left" | "right",
): number {
  const delta = currentX - startX;
  return startWidth + (edge === "right" ? delta : -delta);
}
