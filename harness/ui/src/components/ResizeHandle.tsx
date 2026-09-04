import { onCleanup } from "solid-js";
import { clampPanelWidth, panelWidthFromDrag } from "../lib/layout";

/** Accessible split-view handle. Pointer dragging is the primary desktop gesture; arrows provide precise
 * keyboard control, Home/End jump to the limits, and Enter/double-click restores the designed default. */
export default function ResizeHandle(props: {
  edge: "left" | "right";
  label: string;
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  onChange: (width: number) => void;
}) {
  let stopDrag: (() => void) | undefined;
  const setWidth = (value: number) =>
    props.onChange(clampPanelWidth(value, props.min, props.max));

  const beginDrag = (event: PointerEvent) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    const pointer = event.pointerId;
    const startX = event.clientX;
    // Read the rendered panel width, not only the persisted preference. Responsive max-width may have clamped
    // the panel after the window became narrower; starting from the stale value creates a large dead zone.
    const startWidth = handle.parentElement?.getBoundingClientRect().width ?? props.value;
    const cursor = document.documentElement.style.cursor;
    const selection = document.documentElement.style.userSelect;

    const move = (next: PointerEvent) => {
      if (next.pointerId !== pointer) return;
      setWidth(panelWidthFromDrag(startWidth, startX, next.clientX, props.edge));
    };
    const finish = (next: PointerEvent) => {
      if (next.pointerId !== pointer) return;
      stopDrag?.();
    };
    stopDrag = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      document.documentElement.style.cursor = cursor;
      document.documentElement.style.userSelect = selection;
      if (handle.hasPointerCapture(pointer)) handle.releasePointerCapture(pointer);
      stopDrag = undefined;
    };

    handle.setPointerCapture(pointer);
    document.documentElement.style.cursor = "col-resize";
    document.documentElement.style.userSelect = "none";
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  };

  onCleanup(() => stopDrag?.());

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={props.label}
      aria-valuemin={props.min}
      aria-valuemax={props.max}
      aria-valuenow={props.value}
      tabIndex={0}
      title={props.label}
      onPointerDown={beginDrag}
      onDblClick={() => setWidth(props.defaultValue)}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 12;
        const growKey = props.edge === "right" ? "ArrowRight" : "ArrowLeft";
        const shrinkKey = props.edge === "right" ? "ArrowLeft" : "ArrowRight";
        let next: number | undefined;
        if (event.key === growKey) next = props.value + step;
        else if (event.key === shrinkKey) next = props.value - step;
        else if (event.key === "Home") next = props.min;
        else if (event.key === "End") next = props.max;
        else if (event.key === "Enter") next = props.defaultValue;
        if (next === undefined) return;
        event.preventDefault();
        setWidth(next);
      }}
      class={`group absolute inset-y-0 z-[var(--z-sticky)] block w-3 cursor-col-resize touch-none outline-none ${
        props.edge === "right" ? "right-0 translate-x-1/2" : "left-0 -translate-x-1/2"
      }`}
    >
      <span class="pointer-events-none absolute top-1/2 left-1/2 h-10 w-1 -translate-x-1/2 -translate-y-1/2 rounded-pill bg-line-strong opacity-45 shadow-control transition duration-[var(--dur-fast)] group-hover:bg-accent group-hover:opacity-100 group-focus-visible:bg-accent group-focus-visible:opacity-100" />
    </div>
  );
}
