import { createUniqueId, For, Show, type JSX } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon, { type IconName } from "./../Icon";

/**
 * Mấy mảnh biểu mẫu dùng chung của hai màn hình provider và MCP.
 *
 * Gom lại một chỗ vì cả hai màn hình đều là *cùng một thứ*: một danh sách cấu hình, một
 * hộp thoại sửa, một dãy ô nhập có nhãn. Chép ra hai bản thì hai bản lệch nhau ở đúng
 * những chi tiết không nhìn thấy — nhãn gắn với ô nhập, vòng tiêu điểm, chiều cao control
 * — và lệch ở đó thì chỉ người dùng bàn phím phát hiện ra.
 *
 * Lúc tích hợp thì nâng tệp này lên `components/` cạnh `primitives.tsx`; nó nằm trong
 * `providers/` chỉ vì đợt việc này sở hữu đúng hai thư mục.
 *
 * Mọi prop **chứa JSX** ở đây khai là hàm (`footer`, `aside`). Solid biên dịch prop
 * thành getter, và một prop JSX được đọc hai lần sẽ dựng ra hai bản DOM chồng lên nhau —
 * bản thừa nằm trên và nuốt cú bấm. `children` là ngoại lệ duy nhất: trình biên dịch đã
 * bọc nó lười sẵn.
 */

/** Khung hộp thoại: scrim, bẫy tiêu điểm, Esc đóng, Enter gửi. */
export function DialogShell(props: {
  title: string;
  desc?: string;
  icon: IconName;
  /** Biểu mẫu MCP có hai cột ô nhập, không vừa bề rộng mặc định. */
  wide?: boolean;
  labelledBy?: string;
  onSubmit?: () => void;
  onClose: () => void;
  footer: () => JSX.Element;
  children: JSX.Element;
}) {
  let panel: HTMLDivElement | undefined;
  const titleId = createUniqueId();

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto p-2xl"
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
        class="my-auto flex w-full flex-col gap-(--dialog-gap) rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        classList={{ "max-w-[560px]": !props.wide, "max-w-[720px]": props.wide === true }}
      >
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name={props.icon} size={16} />
          </span>
          <div class="flex min-w-0 flex-1 flex-col gap-3xs">
            <h2 id={titleId} class="m-0 text-md font-semibold text-ink">
              {props.title}
            </h2>
            <Show when={props.desc}>
              {(desc) => <p class="m-0 text-xs text-muted">{desc()}</p>}
            </Show>
          </div>
        </div>

        {/* `<form>` chứ không phải một đống `<div>`: Enter trong bất kỳ ô nào cũng gửi,
            và đó là thứ người dùng bàn phím làm theo phản xạ. */}
        <form
          class="flex flex-col gap-(--dialog-gap)"
          onSubmit={(event) => {
            event.preventDefault();
            props.onSubmit?.();
          }}
        >
          {props.children}
          <div class="flex flex-wrap items-center justify-end gap-sm">{props.footer()}</div>
        </form>
      </div>
    </div>
  );
}

const INPUT_CLASS =
  "h-(--control-h) w-full rounded-btn border border-line bg-bg px-sm text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:cursor-not-allowed disabled:opacity-50";

/** Một ô nhập có nhãn. Nhãn là `<label>` thật, không phải một `<span>` đặt bên trên. */
export function TextField(props: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  hint?: string;
  placeholder?: string;
  type?: "text" | "password";
  mono?: boolean;
  disabled?: boolean;
  invalid?: boolean;
  autocomplete?: string;
  ref?: (el: HTMLInputElement) => void;
}) {
  const id = createUniqueId();
  const hintId = createUniqueId();
  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      <label for={id} class="text-2xs text-faint">
        {props.label}
      </label>
      <input
        id={id}
        ref={props.ref}
        type={props.type ?? "text"}
        value={props.value}
        placeholder={props.placeholder}
        disabled={props.disabled}
        spellcheck={false}
        autocapitalize="off"
        autocomplete={props.autocomplete ?? "off"}
        aria-invalid={props.invalid}
        aria-describedby={props.hint === undefined ? undefined : hintId}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        class={`${INPUT_CLASS} ${props.mono ? "font-mono" : ""}`}
        classList={{ "border-danger": props.invalid === true }}
      />
      <Show when={props.hint}>
        {(hint) => (
          <p id={hintId} class="m-0 text-2xs text-faint">
            {hint()}
          </p>
        )}
      </Show>
    </div>
  );
}

/** Ô nhiều dòng — chỉ dùng cho ô dán JSON, nên không có biến thể nào khác. */
export function TextArea(props: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  placeholder?: string;
  rows?: number;
  invalid?: boolean;
}) {
  const id = createUniqueId();
  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      <label for={id} class="text-2xs text-faint">
        {props.label}
      </label>
      <textarea
        id={id}
        rows={props.rows ?? 5}
        value={props.value}
        placeholder={props.placeholder}
        spellcheck={false}
        aria-invalid={props.invalid}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        class="w-full resize-y rounded-btn border border-line bg-bg px-sm py-2xs font-mono text-2xs text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        classList={{ "border-danger": props.invalid === true }}
      />
    </div>
  );
}

/**
 * Công tắc bật/tắt.
 *
 * `role="switch"` chứ không phải một checkbox trang điểm: trình đọc màn hình đọc "bật"
 * hoặc "tắt", còn checkbox thì đọc "đã chọn" — và "đã chọn" không nói được gì về một
 * server đang chạy hay đang nằm im.
 */
export function Toggle(props: {
  label: string;
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  busy?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      aria-busy={props.busy}
      disabled={props.disabled || props.busy}
      onClick={() => props.onChange(!props.checked)}
      class="relative inline-flex h-5 w-9 shrink-0 items-center rounded-pill border transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-50"
      classList={{
        "border-accent bg-accent": props.checked,
        "border-line-strong bg-surface-soft": !props.checked,
      }}
    >
      <span
        aria-hidden="true"
        class="size-3.5 rounded-pill transition-transform duration-[var(--dur-fast)] motion-reduce:transition-none"
        classList={{
          "translate-x-[18px] bg-on-accent": props.checked,
          "translate-x-[3px] bg-line-strong": !props.checked,
        }}
      />
    </button>
  );
}

/** Dãy nút loại trừ nhau, cùng hình dạng với `radiogroup` của trang Cài đặt. */
export function PillChoice<T extends string>(props: {
  label: string;
  value: T;
  options: { id: T; label: string; icon?: IconName }[];
  onPick: (value: T) => void;
  hint?: string;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <span class="text-2xs text-faint">{props.label}</span>
      <div role="radiogroup" aria-label={props.label} class="flex flex-wrap gap-2xs">
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={props.value === option.id}
              onClick={() => props.onPick(option.id)}
              class="flex items-center gap-2xs rounded-pill border px-md py-2xs text-xs transition-colors duration-[var(--dur-fast)]"
              classList={{
                "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                  props.value !== option.id,
                "border-accent bg-accent-soft text-accent-ink": props.value === option.id,
              }}
            >
              <Show when={option.icon}>{(icon) => <Icon name={icon()} size={13} />}</Show>
              {option.label}
            </button>
          )}
        </For>
      </div>
      <Show when={props.hint}>
        {(hint) => <p class="m-0 text-2xs text-faint">{hint()}</p>}
      </Show>
    </div>
  );
}

export type BannerTone = "info" | "warn" | "danger" | "accent";

/** Một câu cần đọc trước khi bấm. Không phải toast: nó ở lại, vì điều kiện ở lại. */
export function Banner(props: {
  tone: BannerTone;
  icon: IconName;
  title?: string;
  children: JSX.Element;
  role?: "status" | "alert";
}) {
  return (
    <div
      role={props.role}
      class="flex items-start gap-sm rounded-panel border px-sm py-2xs text-2xs"
      classList={{
        "border-line bg-surface-soft text-muted": props.tone === "info",
        "border-warn bg-warn-soft text-warn": props.tone === "warn",
        "border-danger bg-danger-soft text-danger": props.tone === "danger",
        "border-accent bg-accent-soft text-accent-ink": props.tone === "accent",
      }}
    >
      <span class="mt-3xs shrink-0">
        <Icon name={props.icon} size={13} />
      </span>
      <div class="flex min-w-0 flex-col gap-3xs">
        <Show when={props.title}>
          {(title) => <span class="font-medium">{title()}</span>}
        </Show>
        <div class="min-w-0">{props.children}</div>
      </div>
    </div>
  );
}

/**
 * Liên kết ra ngoài.
 *
 * `@tauri-apps/plugin-opener` **chưa được cài** trong `ui/package.json`, và thêm một phụ
 * thuộc npm nằm ngoài phạm vi đợt việc này (nó còn kéo theo cả `Cargo.toml` lẫn danh sách
 * quyền của Tauri). Cho tới lúc đó thì `target="_blank"` là lối duy nhất còn mở; `rel`
 * chặn trang đích với tay ngược vào `window.opener`.
 */
export function ExternalLink(props: { href: string; children: JSX.Element }) {
  return (
    <a
      href={props.href}
      target="_blank"
      rel="noreferrer noopener"
      onClick={(event) => event.stopPropagation()}
      class="inline-flex items-center gap-3xs rounded-btn text-2xs text-accent-ink underline decoration-transparent underline-offset-2 transition-colors duration-[var(--dur-fast)] hover:decoration-current"
    >
      {props.children}
      <Icon name="external" size={11} />
    </a>
  );
}

/** Tiêu đề một khu vực trong trang, cùng nhịp với `SettingsView`. */
export function SectionHead(props: {
  title: string;
  desc: string;
  actions?: () => JSX.Element;
}) {
  return (
    <div class="flex flex-wrap items-end justify-between gap-sm">
      <div class="flex min-w-0 flex-col gap-3xs">
        <h2 class="m-0 text-md font-semibold text-ink">{props.title}</h2>
        <p class="m-0 text-xs text-muted">{props.desc}</p>
      </div>
      <Show when={props.actions}>{(render) => <div class="flex gap-sm">{render()()}</div>}</Show>
    </div>
  );
}

/** Nút chính/phụ của hộp thoại — cùng chiều cao, cùng bo góc, khác trọng lượng. */
export function Button(props: {
  label: string;
  variant?: "primary" | "ghost" | "outline";
  type?: "button" | "submit";
  icon?: IconName;
  disabled?: boolean;
  busy?: boolean;
  onClick?: () => void;
}) {
  const variant = () => props.variant ?? "primary";
  return (
    <button
      type={props.type ?? "button"}
      disabled={props.disabled || props.busy}
      aria-busy={props.busy}
      onClick={() => props.onClick?.()}
      class="inline-flex h-(--control-h) items-center gap-2xs rounded-btn px-md text-xs font-medium transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40"
      classList={{
        "bg-accent text-on-accent enabled:hover:bg-accent-hover": variant() === "primary",
        "border border-line-strong text-text enabled:hover:bg-surface-hover": variant() === "outline",
        "text-muted enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink":
          variant() === "ghost",
      }}
    >
      <Show when={props.icon}>{(icon) => <Icon name={icon()} size={13} />}</Show>
      {props.label}
    </button>
  );
}
