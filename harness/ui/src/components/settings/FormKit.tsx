import { createSignal, createUniqueId, For, Show, type JSX } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon, { type IconName } from "./../Icon";

/**
 * Ngôn ngữ chung của **mọi** trang cài đặt: hàng, nhóm hàng, công tắc, ô chọn, hộp thoại.
 *
 * Gom lại một chỗ vì mọi trang cài đặt đều là *cùng một thứ*: một danh sách cấu hình, một
 * hộp thoại sửa, một dãy ô nhập có nhãn. Chép ra nhiều bản thì các bản lệch nhau ở đúng
 * những chi tiết không nhìn thấy — nhãn gắn với ô nhập, vòng tiêu điểm, chiều cao control
 * — và lệch ở đó thì chỉ người dùng bàn phím phát hiện ra.
 *
 * Tệp nằm trong `settings/` chứ không trong `providers/` vì đó là sự thật: hai trang
 * provider chỉ là hai trong bảy trang dùng nó. Hai kiểu hàng trong cùng một màn hình cài
 * đặt đọc ra là hai màn hình bị dán vào nhau, nên đây là bộ **duy nhất** được dùng.
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
  /** Đoạn dài đằng sau `desc`, cất trong `InfoDot` cạnh tiêu đề hộp thoại. */
  more?: string;
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
            <h2 id={titleId} class="m-0 flex items-center gap-2xs text-md font-semibold text-ink">
              {props.title}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
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
  /** Câu dài đứng sau `hint`, cất trong `InfoDot` cạnh nhãn thay vì trải dưới ô. */
  more?: string;
  placeholder?: string;
  type?: "text" | "password";
  mono?: boolean;
  disabled?: boolean;
  invalid?: boolean;
  autocomplete?: string;
  /**
   * Giấu nhãn **khỏi mắt**, không khỏi trình đọc màn hình.
   *
   * Dùng khi ô nằm trong một `<Row>` đã mang nhãn ở cột trái: vẽ nhãn lần thứ hai thì
   * hàng cao gấp đôi, còn bỏ hẳn `<label>` thì ô mất tên với người dùng bàn phím.
   */
  hideLabel?: boolean;
  ref?: (el: HTMLInputElement) => void;
}) {
  const id = createUniqueId();
  const hintId = createUniqueId();
  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      <Show
        when={props.more !== undefined && props.hideLabel !== true}
        fallback={
          <label for={id} class={props.hideLabel === true ? "sr-only" : "text-2xs text-faint"}>
            {props.label}
          </label>
        }
      >
        <span class="flex items-center gap-2xs text-2xs text-faint">
          <label for={id}>{props.label}</label>
          <InfoDot text={props.more ?? ""} label={`Về ${props.label}`} />
        </span>
      </Show>
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
  /** Câu dài đứng sau `hint`, cất trong `InfoDot` cạnh nhãn của nhóm. */
  more?: string;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <span class="flex items-center gap-2xs text-2xs text-faint">
        {props.label}
        <Show when={props.more}>
          {(more) => <InfoDot text={more()} label={`Về ${props.label}`} />}
        </Show>
      </span>
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

/**
 * Danh sách cài đặt **gọn** — hình dạng của hộp thoại Cài đặt trong ChatGPT.
 *
 * Ở đó mỗi mục là một hàng chứ không phải một thẻ: nhãn bên trái, điều khiển bên phải,
 * một dòng mô tả nhỏ dưới nhãn khi cần, và các hàng chỉ ngăn nhau bằng một nét chỉ. Ta
 * theo hình dạng đó vì cùng một lý do họ chọn nó: trang cài đặt là một danh sách *dài*
 * gồm nhiều thứ không liên quan nhau, và đóng khung mỗi thứ vào một thẻ riêng làm mắt
 * phải nhảy qua bốn cạnh viền trước mỗi lần đọc một nhãn.
 *
 * Không tự dựng khung cuộn ở đây: khung cuộn là của trang cha. Một vùng cuộn lồng trong
 * một vùng cuộn là hai thanh cuộn, và người dùng luôn lăn nhầm cái ngoài.
 */
export function RowGroup(props: { children: JSX.Element }) {
  return (
    <div class="flex flex-col divide-y divide-line overflow-hidden rounded-card border border-line bg-surface">
      {props.children}
    </div>
  );
}

/**
 * Một hàng cài đặt.
 *
 * `control` và `below` khai là **hàm** vì chúng chứa JSX: Solid biên dịch prop thành
 * getter, và một prop JSX bị đọc hai lần dựng ra hai bản DOM chồng lên nhau.
 */
export function Row(props: {
  label: string;
  desc?: string;
  /** Tên server MCP là một định danh — nó xuất hiện nguyên văn trong tiền tố tên tool. */
  labelMono?: boolean;
  /** Biểu tượng đứng đầu hàng. Nó thay phần nghĩa mà `desc` một dòng không chứa nổi. */
  icon?: IconName;
  /** Đoạn giải thích dài, cất trong `InfoDot` cạnh nhãn. */
  more?: string;
  /** Chấm trạng thái hoặc biểu tượng đứng trước nhãn. */
  lead?: () => JSX.Element;
  /** Điều khiển ở cột phải — công tắc, nút, ô chọn. */
  control?: () => JSX.Element;
  /** Phần bung ra dưới hàng: cảnh báo, danh sách, chi tiết của chính hàng đó. */
  below?: () => JSX.Element;
  /** Hàng mờ đi khi mục của nó đang tắt, nhưng vẫn phải đọc được để bật lại. */
  dim?: boolean;
}) {
  return (
    <div
      class="flex flex-col gap-2xs px-(--card-pad-x) py-sm transition-colors duration-[var(--dur-fast)]"
      classList={{ "opacity-70": props.dim === true }}
    >
      <div class="flex flex-wrap items-center gap-md">
        <Show when={props.lead}>{(render) => <>{render()()}</>}</Show>
        <Show when={props.icon}>
          {(icon) => (
            <span class="grid size-7 shrink-0 place-items-center rounded-panel bg-surface-soft text-muted">
              <Icon name={icon()} size={14} />
            </span>
          )}
        </Show>
        <div class="flex min-w-0 flex-1 flex-col gap-3xs">
          <span
            class="flex min-w-0 items-center gap-2xs text-xs font-medium text-ink"
            classList={{ "font-mono": props.labelMono === true }}
          >
            {props.label}
            <Show when={props.more}>{(more) => <InfoDot text={more()} label={`Về ${props.label}`} />}</Show>
          </span>
          <Show when={props.desc}>
            {(desc) => <p class="m-0 text-2xs text-muted">{desc()}</p>}
          </Show>
        </div>
        <Show when={props.control}>
          {(render) => <div class="flex shrink-0 items-center gap-2xs">{render()()}</div>}
        </Show>
      </div>
      <Show when={props.below}>{(render) => <>{render()()}</>}</Show>
    </div>
  );
}

/**
 * Ô chọn một-trong-nhiều, cột phải của một hàng.
 *
 * `<select>` thật của trình duyệt chứ không phải một menu tự vẽ: danh sách mô hình dài
 * tuỳ máy chủ, và một danh sách tự vẽ vừa phải tự lo cuộn, tự lo bàn phím, vừa phải tự
 * lo cả việc đóng khi cuộn trang — ba thứ trình duyệt đã làm đúng sẵn.
 */
export function Select(props: {
  label: string;
  value: string;
  options: { id: string; label: string }[];
  onPick: (value: string) => void;
  disabled?: boolean;
  mono?: boolean;
  /**
   * Chiếm trọn bề ngang thay vì dừng ở 280px.
   *
   * Bề rộng mặc định hợp với một `<Row>` của trang cài đặt, nơi ô điều khiển đứng ở cột
   * phải cạnh một cột nhãn. Trong một hộp thoại thì nó lại là ô duy nhất trên dòng, và
   * một ô hẹp hơn ô ngay trên nó đọc như một ô bị vỡ.
   */
  full?: boolean;
}) {
  const WIDTH = props.full === true ? "w-full" : "max-w-[280px]";
  return (
    <select
      aria-label={props.label}
      value={props.value}
      disabled={props.disabled}
      onChange={(event) => props.onPick(event.currentTarget.value)}
      class={`h-(--control-h) ${WIDTH} min-w-0 truncate rounded-btn border border-line bg-bg px-sm text-xs text-text outline-none transition-colors duration-[var(--dur-fast)] focus:border-accent disabled:cursor-not-allowed disabled:opacity-50`}
      classList={{ "font-mono": props.mono === true }}
    >
      {/* `selected` đặt trên từng `<option>` chứ không chỉ dựa vào `value` của `<select>`:
          danh sách được dựng lại mỗi khi máy chủ trả về mô hình khác, và một `<select>`
          được gán `value` trước lúc có `<option>` tương ứng sẽ tự rơi về mục đầu tiên. */}
      <For each={props.options}>
        {(option) => (
          <option value={option.id} selected={option.id === props.value}>
            {option.label}
          </option>
        )}
      </For>
    </select>
  );
}

export type BannerTone = "info" | "warn" | "danger" | "accent";

/** Một câu cần đọc trước khi bấm. Không phải toast: nó ở lại, vì điều kiện ở lại. */
export function Banner(props: {
  tone: BannerTone;
  icon: IconName;
  title?: string;
  /** Đoạn dài đằng sau lời cảnh báo, cất trong `InfoDot` cạnh tiêu đề. */
  more?: string;
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
          {(title) => (
            <span class="flex items-center gap-2xs font-medium">
              {title()}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </span>
          )}
        </Show>
        <div class="flex min-w-0 items-start gap-2xs">
          <span class="min-w-0">{props.children}</span>
          {/* Không tiêu đề thì chấm hỏi đi cùng thân: gắn nó vào `title` và chỉ vậy thôi
              thì một banner không tiêu đề nuốt mất `more` mà không báo gì. */}
          <Show when={props.more !== undefined && props.title === undefined}>
            <InfoDot text={props.more ?? ""} />
          </Show>
        </div>
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


/**
 * Dấu chấm hỏi cạnh một nhãn: câu giải thích dài nằm trong đây, không nằm trên trang.
 *
 * Mọi nhãn và mô tả trên màn hình bị giới hạn một câu ngắn, vì một trang cài đặt được
 * *quét* chứ không được đọc. Nhưng lý do đằng sau một công tắc vẫn phải còn ở đâu đó —
 * mất nó thì người dùng bật/tắt theo phỏng đoán. Chỗ đó là đây: chữ đầy đủ hiện khi rê
 * chuột hoặc khi tiêu điểm bàn phím rơi vào, và luôn nối với nhãn qua `aria-describedby`
 * nên trình đọc màn hình đọc được kể cả khi không có con trỏ nào.
 */
export function InfoDot(props: { text: string; label?: string }) {
  const [open, setOpen] = createSignal(false);
  const id = createUniqueId();
  return (
    <span class="relative inline-flex shrink-0 items-center">
      <button
        type="button"
        aria-label={props.label ?? "Giải thích"}
        aria-describedby={id}
        aria-expanded={open()}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((was) => !was)}
        class="grid size-4 place-items-center rounded-pill text-faint transition-colors duration-[var(--dur-fast)] hover:text-ink"
      >
        <Icon name="info" size={13} />
      </button>
      <span
        id={id}
        role="tooltip"
        class="pointer-events-none absolute bottom-[calc(100%+6px)] left-1/2 z-40 w-64 -translate-x-1/2 rounded-panel border border-line bg-surface px-sm py-2xs text-2xs leading-relaxed text-muted shadow-pop transition-opacity duration-[var(--dur-fast)]"
        classList={{ "opacity-0": !open(), "opacity-100": open() }}
        aria-hidden={!open()}
      >
        {props.text}
      </span>
    </span>
  );
}

/** Tiêu đề một khu vực trong trang, cùng nhịp với `SettingsView`. */
export function SectionHead(props: {
  title: string;
  desc: string;
  /** Biểu tượng của khu vực. Nó gánh phần nghĩa mà câu mô tả một dòng phải bỏ lại. */
  icon?: IconName;
  /** Đoạn dài đằng sau tiêu đề, cất trong `InfoDot` thay vì trải ra trang. */
  more?: string;
  actions?: () => JSX.Element;
}) {
  return (
    <div class="flex flex-wrap items-end justify-between gap-sm">
      <div class="flex min-w-0 flex-1 items-start gap-sm">
        <Show when={props.icon}>
          {(icon) => (
            <span class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
              <Icon name={icon()} size={15} />
            </span>
          )}
        </Show>
        <div class="flex min-w-0 flex-col gap-3xs">
          <h2 class="m-0 flex items-center gap-2xs text-md font-semibold text-ink">
            {props.title}
            <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
          </h2>
          <p class="m-0 text-xs text-muted">{props.desc}</p>
        </div>
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
