import { onCleanup, onMount } from "solid-js";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "textarea:not([disabled])",
  "select:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

/**
 * Giam tiêu điểm bàn phím trong một hộp thoại.
 *
 * `aria-modal` nói với trình đọc màn hình rằng phần còn lại của trang không tồn tại,
 * nhưng nó KHÔNG ngăn Tab đi ra ngoài — người dùng bàn phím sẽ lạc vào một cây DOM mà
 * trình đọc màn hình vừa bảo là không có. Nên phải tự vòng Tab lại.
 *
 * `onEscape` cũng nằm ở đây thay vì ở từng hộp thoại: mọi hộp thoại đều phải đóng bằng
 * Esc, và quên một chỗ là một cái bẫy im lặng.
 */
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
    // Trả tiêu điểm về chỗ cũ. Không làm thì sau khi đóng hộp thoại, Tab tiếp theo bắt
    // đầu lại từ đầu trang.
    restore?.focus();
  });
}
