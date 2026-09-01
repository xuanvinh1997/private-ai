import { createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import Icon, { type IconName } from "./Icon";

export interface MenuItem {
  id: string;
  label: string;
  icon: IconName;
  /**
   * Dòng phụ nói ra **hậu quả** của hàng này.
   *
   * Có mặt vì hai hàng dễ bấm nhầm nhất — "đóng dự án" và "bỏ khỏi danh sách" — chỉ phân
   * biệt được bằng hậu quả, và hậu quả nằm trong hộp xác nhận thì đã muộn: người ta quyết
   * định bấm hay không *trước* khi hộp đó kịp hiện ra.
   */
  hint?: string;
  danger?: boolean;
  /**
   * Khoá hàng lại thay vì cắt nó đi.
   *
   * Một hàng biến mất để lại câu hỏi "cái đó đâu rồi"; một hàng khoá cùng `hint` nói ra
   * luôn vì sao chưa bấm được.
   */
  disabled?: boolean;
  onSelect: () => void;
}

/**
 * Menu ngữ cảnh nhỏ.
 *
 * Nút mở nó hiện ra khi rê chuột, nhưng menu **không** mở bằng việc rê chuột: một menu
 * tự bung ra khi con trỏ đi ngang qua là thứ người ta phải né, không phải thứ người ta
 * dùng. Mở bằng cú bấm, bằng Enter, hoặc bằng chuột phải lên chính hàng đó.
 *
 * Mũi tên lên/xuống đi trong danh sách và Esc đóng lại, nên mọi thứ làm được bằng chuột
 * ở đây đều làm được bằng bàn phím — kể cả khi nút mở chỉ hiện lúc rê chuột, vì tiêu
 * điểm bàn phím cũng làm nó hiện.
 */
export function Menu(props: {
  items: MenuItem[];
  label: string;
  /** Cho hàng cha biết menu đang mở, để nó giữ nút "…" hiện ra. */
  onOpenChange?: (open: boolean) => void;
  open?: boolean;
  onRequestClose?: () => void;
  /** `pill` hiện luôn giá trị đang chọn: một lựa chọn phải đọc được mà không cần mở ra. */
  variant?: "icon" | "pill";
  icon?: IconName;
  text?: string;
  tone?: "neutral" | "warn";
  align?: "left" | "right";
  /** Menu ở đáy màn hình phải bung lên, nếu không nó rơi ra ngoài cửa sổ. */
  placement?: "down" | "up";
}) {
  const [open, setOpen] = createSignal(false);
  const id = createUniqueId();
  let popup: HTMLDivElement | undefined;
  let trigger: HTMLButtonElement | undefined;

  const isOpen = () => props.open ?? open();

  const setState = (next: boolean) => {
    if (props.open !== undefined) {
      if (!next) props.onRequestClose?.();
    } else {
      setOpen(next);
    }
    props.onOpenChange?.(next);
  };

  // Bấm ra ngoài đóng menu. Nghe ở pha bắt (capture) để cú bấm đó không kịp kích hoạt
  // một hàng khác trước khi menu biết mình phải đóng.
  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    setState(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  const move = (delta: number) => {
    // Bỏ qua hàng bị khoá: mũi tên dừng trên một hàng không bấm được là một ngõ cụt câm.
    const buttons = [
      ...(popup?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? []),
    ];
    if (buttons.length === 0) return;
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = (at + delta + buttons.length) % buttons.length;
    buttons[next]?.focus();
  };

  const close = (restore: boolean) => {
    setState(false);
    if (restore) trigger?.focus();
  };

  return (
    <div class="relative">
      <button
        ref={trigger}
        type="button"
        aria-label={props.label}
        aria-haspopup="menu"
        aria-expanded={isOpen()}
        aria-controls={id}
        onClick={(event) => {
          event.stopPropagation();
          setState(!isOpen());
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setState(true);
            queueMicrotask(() => move(1));
          }
        }}
        class="flex items-center gap-3xs rounded-pill transition-colors duration-[var(--dur-fast)]"
        classList={{
          "size-6 justify-center text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
            (props.variant ?? "icon") === "icon",
          "h-(--control-h) px-sm text-2xs": props.variant === "pill",
          "bg-[var(--overlay-faint)] text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
            props.variant === "pill" && (props.tone ?? "neutral") === "neutral",
          "bg-warn-soft text-warn hover:bg-warn-soft": props.variant === "pill" && props.tone === "warn",
        }}
      >
        <Show when={props.variant === "pill"} fallback={<Icon name="more" size={14} />}>
          <Show when={props.icon}>{(icon) => <Icon name={icon()} size={13} />}</Show>
          <span class="max-w-40 truncate">{props.text}</span>
          <Icon name="chevron-down" size={12} />
        </Show>
      </button>

      <Show when={isOpen()}>
        <div
          ref={popup}
          id={id}
          role="menu"
          aria-label={props.label}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              close(true);
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            }
          }}
          class="absolute z-40 flex flex-col gap-3xs rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
          classList={{
            // Menu có dòng phụ phải rộng hơn: một câu giải thích bị bẻ thành bốn dòng hai
            // chữ thì không ai đọc, và cả lý do để nó tồn tại mất theo.
            "min-w-40": !props.items.some((item) => item.hint !== undefined),
            "w-56": props.items.some((item) => item.hint !== undefined),
            "right-0": (props.align ?? "right") === "right",
            "left-0": props.align === "left",
            "top-full mt-3xs": (props.placement ?? "down") === "down",
            "bottom-full mb-3xs": props.placement === "up",
          }}
        >
          <For each={props.items}>
            {(item) => (
              <button
                type="button"
                role="menuitem"
                disabled={item.disabled}
                onClick={(event) => {
                  event.stopPropagation();
                  close(false);
                  item.onSelect();
                }}
                class="flex w-full flex-col gap-3xs rounded-btn px-sm py-2xs text-left text-xs transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-60"
                classList={{
                  "text-text enabled:hover:bg-[var(--overlay-hover)]": !item.danger,
                  "text-danger enabled:hover:bg-danger-soft": item.danger === true,
                }}
              >
                <span class="flex items-center gap-sm">
                  <Icon name={item.icon} size={14} />
                  {item.label}
                </span>
                <Show when={item.hint}>
                  {(hint) => <span class="text-2xs text-faint">{hint()}</span>}
                </Show>
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

export default Menu;
