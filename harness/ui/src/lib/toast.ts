import { createSignal } from "solid-js";
import { upsertAppNotification } from "./notifications";

/** Floating notices for things that just happened; the composer status line is for conditions that persist. */

export type ToastKind = "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

/** Three at a time; a fourth pushes out the oldest, because a taller stack stops being read at all. */
const MAX = 3;

/** Eight seconds, errors included: each one describes a gesture just made, and a close button handles the rest. */
const LIFETIME_MS = 8_000;

const [toasts, setToasts] = createSignal<Toast[]>([]);
export { toasts };

let seq = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function forget(id: number) {
  const timer = timers.get(id);
  if (timer !== undefined) clearTimeout(timer);
  timers.delete(id);
}

function arm(id: number) {
  forget(id);
  timers.set(
    id,
    setTimeout(() => {
      forget(id);
      setToasts((all) => all.filter((toast) => toast.id !== id));
    }, LIFETIME_MS),
  );
}

/** Push a notice; text identical to a visible one only resets its timer instead of stacking a second card. */
export function notify(kind: ToastKind, text: string): void {
  const trimmed = text.trim();
  if (trimmed === "") return;

  const existing = toasts().find((toast) => toast.kind === kind && toast.text === trimmed);
  if (existing !== undefined) {
    arm(existing.id);
    return;
  }

  const toast: Toast = { id: ++seq, kind, text: trimmed };
  upsertAppNotification(
    {
      id: `toast:${toast.id}`,
      tone: kind,
      title: "",
      message: trimmed,
      dismissible: true,
    },
    true,
  );
  setToasts((all) => {
    const next = [...all, toast];
    // Trim from the front: oldest first, since the newest describes the gesture just made.
    for (const dropped of next.slice(0, Math.max(0, next.length - MAX))) forget(dropped.id);
    return next.slice(-MAX);
  });
  arm(toast.id);
}

export function dismissToast(id: number): void {
  forget(id);
  setToasts((all) => all.filter((toast) => toast.id !== id));
}
