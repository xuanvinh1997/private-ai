import { onCleanup, onMount } from "solid-js";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "textarea:not([disabled])",
  "select:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

/** Trap keyboard focus in a dialog: aria-modal hides the page from screen readers but does not stop Tab. */
export function useFocusTrap(container: () => HTMLElement | undefined, onEscape: () => void) {
  let restore: HTMLElement | null = null;

  const items = (): HTMLElement[] => {
    const root = container();
    if (!root) return [];
    return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
      (el) => el.offsetParent !== null || el === document.activeElement,
    );
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onEscape();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = items();
    if (focusable.length === 0) return;
    const first = focusable[0]!;
    const last = focusable[focusable.length - 1]!;
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !container()?.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  onMount(() => {
    restore = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    items()[0]?.focus();
    document.addEventListener("keydown", onKeyDown, true);
  });

  onCleanup(() => {
    document.removeEventListener("keydown", onKeyDown, true);
    // Restore focus; otherwise the next Tab after closing starts from the top of the page.
    restore?.focus();
  });
}
