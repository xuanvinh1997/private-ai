import { createUniqueId, Show, type JSX } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon, { type IconName } from "../Icon";

/**
 * Vỏ chung cho các hộp thoại của màn hình dự án và màn hình thư viện.
 *
 * Gom lại không phải vì cái khung — cái khung là mười dòng. Gom lại vì ba thứ dễ quên
 * nhất của một hộp thoại đều vô hình: bẫy tiêu điểm, Esc đóng, và trả tiêu điểm về đúng
 * chỗ cũ khi đóng. Bốn hộp thoại tự viết bốn lần là bốn cơ hội quên một trong ba thứ đó,
 * và cả ba đều không lộ ra khi thử bằng chuột.
 *
 * `footer` khai là **hàm** chứ không nhận JSX trực tiếp: Solid biên dịch prop chứa JSX
 * thành getter, nên một prop JSX bị đọc hai lần sẽ dựng hai bản nút — bản thừa nằm đè
 * lên bản kia và nuốt mất cú bấm.
 */
export default function DialogShell(props: {
  icon: IconName;
  title: string;
  desc?: string;
  tone?: "accent" | "danger";
  /** Có việc đang chạy trong hộp thoại; trình đọc màn hình cần biết để không đọc vội. */
  busy?: boolean;
  width?: "md" | "lg";
  onClose: () => void;
  children: JSX.Element;
  footer: () => JSX.Element;
}) {
  let panel: HTMLDivElement | undefined;
  const titleId = createUniqueId();
  const descId = createUniqueId();

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-50 flex justify-center overflow-y-auto p-4xl"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={props.desc === undefined ? undefined : descId}
        aria-busy={props.busy === true ? "true" : "false"}
        class="my-auto flex w-full flex-col gap-md rounded-card border border-line bg-surface p-(--dialog-pad-x) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        classList={{
          "max-w-[560px]": (props.width ?? "md") === "md",
          "max-w-[680px]": props.width === "lg",
        }}
      >
        <div class="flex items-start gap-sm">
          <span
            class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel"
            classList={{
              "bg-accent-soft text-accent-ink": (props.tone ?? "accent") === "accent",
              "bg-danger-soft text-danger": props.tone === "danger",
            }}
          >
            <Icon name={props.icon} size={16} />
          </span>
          <div class="flex min-w-0 flex-col gap-3xs">
            <h2 id={titleId} class="m-0 text-md font-semibold text-ink">
              {props.title}
            </h2>
            <Show when={props.desc}>
              {(text) => (
                <p id={descId} class="m-0 text-xs text-muted">
                  {text()}
                </p>
              )}
            </Show>
          </div>
        </div>

        {props.children}

        <div class="flex flex-wrap items-center justify-end gap-sm">{props.footer()}</div>
      </div>
    </div>
  );
}

/**
 * Ba kiểu nút dùng lại trong hộp thoại và trên màn hình dự án.
 *
 * Không phải một hệ thống nút — chỉ là ba chỗ mà lặp lại chuỗi class dài này sẽ khiến
 * hai nút cùng vai trò trông khác nhau sau vài lần sửa.
 */
export function Button(props: {
  children: JSX.Element;
  onClick?: () => void;
  variant?: "primary" | "ghost" | "outline" | "danger";
  disabled?: boolean;
  icon?: IconName;
  label?: string;
  title?: string;
  type?: "button" | "submit";
}) {
  const variant = () => props.variant ?? "ghost";
  return (
    <button
      type={props.type ?? "button"}
      onClick={() => props.onClick?.()}
      disabled={props.disabled}
      aria-label={props.label}
      title={props.title}
      class="flex h-(--control-h) shrink-0 items-center gap-2xs rounded-btn px-md text-xs transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40"
      classList={{
        "bg-accent font-medium text-on-accent enabled:hover:bg-accent-hover":
          variant() === "primary",
        "text-muted enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink":
          variant() === "ghost",
        "border border-line text-text enabled:hover:border-accent enabled:hover:bg-accent-soft enabled:hover:text-accent-ink":
          variant() === "outline",
        "bg-danger font-medium text-on-accent enabled:hover:opacity-90": variant() === "danger",
      }}
    >
      <Show when={props.icon}>{(name) => <Icon name={name()} size={14} />}</Show>
      {props.children}
    </button>
  );
}
