import { createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import Icon, { type IconName } from "./Icon";

export interface MenuItem {
  id: string;
  label: string;
  icon: IconName;
  /** A choice menu exposes its current value as radio state, not only as a differently coloured row. */
  selected?: boolean;
  /** Secondary line naming this row's *consequence*; the two most confusable rows differ only by consequence. */
  hint?: string;
  danger?: boolean;
  /** Disable a row instead of removing it: a missing row raises a question, a disabled one with a `hint` answers it. */
  disabled?: boolean;
  onSelect: () => void;
}

/** Small context menu; the trigger appears on hover but the menu opens only on click, Enter or right-click,
 * and arrow keys plus Esc make everything reachable from the keyboard. */
export function Menu(props: {
  items: MenuItem[];
  label: string;
  /** Tells the parent row the menu is open, so it keeps the trigger visible. */
  onOpenChange?: (open: boolean) => void;
  open?: boolean;
  onRequestClose?: () => void;
  /** `pill` shows the current value: a selection must be readable without opening the menu. */
  variant?: "icon" | "pill";
  /** Match a standalone small icon button when an icon menu sits beside one in a toolbar. */
  size?: "sm";
  icon?: IconName;
  text?: string;
  tone?: "neutral" | "warn";
  align?: "left" | "right";
  /** A menu at the bottom of the screen must open upward or it falls outside the window. */
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

  // Click outside closes; listening in the capture phase so that click cannot trigger another row first.
  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    setState(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  const move = (delta: number) => {
    // Skip disabled rows: an arrow key landing on an unclickable row is a silent dead end.
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
        class="flex items-center gap-3xs transition-colors duration-[var(--dur-fast)]"
        classList={{
          "justify-center rounded-icon text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
            (props.variant ?? "icon") === "icon",
          "bg-accent-soft text-accent-ink":
            (props.variant ?? "icon") === "icon" && isOpen(),
          "size-6": (props.variant ?? "icon") === "icon" && props.size !== "sm",
          "size-(--icon-control-h)":
            (props.variant ?? "icon") === "icon" && props.size === "sm",
          "h-(--control-h) rounded-pill border px-sm text-xs shadow-control": props.variant === "pill",
          "border-line bg-surface-soft text-muted hover:border-line-strong hover:bg-surface hover:text-ink":
            props.variant === "pill" && (props.tone ?? "neutral") === "neutral",
          "border-warn/40 bg-warn-soft text-warn hover:border-warn/70 hover:bg-warn-soft":
            props.variant === "pill" && props.tone === "warn",
        }}
      >
        <Show
          when={props.variant === "pill"}
          fallback={<Icon name={props.icon ?? "more"} size={props.size === "sm" ? 13 : 14} />}
        >
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
          class="absolute z-[var(--z-popover)] flex flex-col gap-3xs rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
          classList={{
            // A menu with hints must be wider: an explanation broken into four two-word lines goes unread.
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
                role={item.selected === undefined ? "menuitem" : "menuitemradio"}
                aria-checked={item.selected}
                disabled={item.disabled}
                onClick={(event) => {
                  event.stopPropagation();
                  // `true`: restore focus to the trigger *before* running the action, since the menu item is
                  // gone by the time a dialog opened here closes and its focus trap restores.
                  close(true);
                  item.onSelect();
                }}
                class="flex w-full flex-col gap-3xs rounded-btn px-sm py-2xs text-left text-xs transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-60"
                classList={{
                  "text-text enabled:hover:bg-[var(--overlay-hover)]":
                    !item.danger && item.selected !== true,
                  "text-danger enabled:hover:bg-danger-soft": item.danger === true,
                  "bg-accent-soft text-accent-ink enabled:hover:bg-accent-soft":
                    item.selected === true,
                }}
              >
                <span class="flex items-center gap-sm">
                  <Icon name={item.icon} size={14} />
                  {item.label}
                </span>
                <Show when={item.hint}>
                  {(hint) => <span class="text-xs text-faint">{hint()}</span>}
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
