import { createSignal, createUniqueId, Show, type JSX } from "solid-js";
import { useTranscriptActions } from "../lib/transcriptActions";
import Icon, { type IconName } from "./Icon";

/** Trạng thái một tool call. `warn` dành cho lệnh xong nhưng exit code khác 0. */
export type DotState = "running" | "ok" | "error" | "warn";

export function StateDot(props: { state: DotState; label?: string }) {
  return (
    <span
      role="img"
      aria-label={props.label ?? LABEL[props.state]}
      title={props.label ?? LABEL[props.state]}
      class="size-1.5 shrink-0 rounded-pill"
      classList={{
        // Chấm "đang chạy" thở nhẹ: một tool treo và một tool đã xong trông giống hệt
        // nhau nếu chấm đứng im, và chờ nhầm là cách người dùng mất niềm tin nhanh nhất.
        "bg-muted motion-safe:animate-pulse": props.state === "running",
        "bg-success": props.state === "ok",
        "bg-warn": props.state === "warn",
        "bg-danger": props.state === "error",
      }}
    />
  );
}

const LABEL: Record<DotState, string> = {
  running: "đang chạy",
  ok: "xong",
  warn: "xong, có cảnh báo",
  error: "lỗi",
};

type TipSide = "right" | "bottom" | "left";

/**
 * Nút chỉ có biểu tượng.
 *
 * `aria-label` là bắt buộc trong chữ ký chứ không phải tuỳ chọn: một nút biểu tượng
 * không nhãn là một nút không tồn tại với trình đọc màn hình, và "quên nhãn" là lỗi phải
 * chặn ở kiểu, không phải ở khâu rà soát.
 *
 * Chú giải là một `<span>` bình thường được `aria-hidden`, không phải `title` — `title`
 * chỉ hiện sau độ trễ của hệ điều hành và không bao giờ hiện khi đi bằng bàn phím.
 */
export function IconButton(props: {
  icon: IconName;
  label: string;
  onClick?: (event: MouseEvent) => void;
  size?: "sm" | "md" | "lg";
  active?: boolean;
  danger?: boolean;
  disabled?: boolean;
  expanded?: boolean;
  controls?: string;
  keys?: string;
  tip?: TipSide;
  ref?: (el: HTMLButtonElement) => void;
}) {
  const box = () => (props.size === "lg" ? "size-10" : props.size === "sm" ? "size-6" : "size-8");
  const glyph = () => (props.size === "lg" ? 18 : props.size === "sm" ? 13 : 15);
  return (
    <span class="group/tip relative inline-flex shrink-0">
      <button
        ref={props.ref}
        type="button"
        onClick={(event) => props.onClick?.(event)}
        disabled={props.disabled}
        aria-label={props.label}
        aria-pressed={props.active}
        aria-expanded={props.expanded}
        aria-controls={props.controls}
        aria-keyshortcuts={props.keys}
        class={`grid ${box()} place-items-center rounded-icon transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40`}
        classList={{
          "text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
            !props.active && !props.danger,
          "bg-accent-soft text-accent-ink": props.active === true,
          "text-danger hover:bg-danger-soft": props.danger === true,
        }}
      >
        <Icon name={props.icon} size={glyph()} />
      </button>
      <Tip side={props.tip ?? "bottom"}>{props.label}</Tip>
    </span>
  );
}

/** Chú giải dùng chung. Chỉ là trang trí — nội dung thật nằm ở `aria-label` của nút. */
export function Tip(props: { side: TipSide; children: JSX.Element }) {
  return (
    <span
      aria-hidden="true"
      class="pointer-events-none absolute z-50 hidden rounded-btn bg-ink px-2xs py-3xs text-2xs whitespace-nowrap text-bg opacity-0 shadow-float transition-opacity duration-[var(--dur-fast)] group-hover/tip:opacity-100 group-focus-within/tip:opacity-100 md:block"
      classList={{
        "left-full top-1/2 ml-sm -translate-y-1/2": props.side === "right",
        "right-full top-1/2 mr-sm -translate-y-1/2": props.side === "left",
        "top-full left-1/2 mt-2xs -translate-x-1/2": props.side === "bottom",
      }}
    >
      {props.children}
    </span>
  );
}

/**
 * Một khối gập được.
 *
 * `aria-controls` cần một id thật, và id phải sinh ra ở phía client vì có nhiều khối
 * cùng loại trên màn hình — `createUniqueId` lo phần đó.
 */
export function Disclosure(props: {
  label: string;
  hint?: string;
  open?: boolean;
  children: JSX.Element;
}) {
  const [open, setOpen] = createSignal(props.open ?? false);
  const id = createUniqueId();
  return (
    <div class="flex flex-col">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open()}
        aria-controls={id}
        class="flex items-center gap-2xs self-start rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
      >
        <Icon
          name="chevron-right"
          size={12}
          class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
        />
        {props.label}
        <Show when={props.hint}>
          <span class="text-faint">{props.hint}</span>
        </Show>
      </button>
      <div id={id} hidden={!open()} class="mt-2xs">
        {props.children}
      </div>
    </div>
  );
}

/**
 * Nút chép. Đổi biểu tượng 1,5 giây rồi tự trả về — không có toast, vì một thao tác đã
 * thành công không đáng để chiếm một góc màn hình.
 */
export function CopyButton(props: { text: () => string; label?: string }) {
  const [done, setDone] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.text());
      setDone(true);
      setTimeout(() => setDone(false), 1500);
    } catch (err) {
      console.error("không chép được", err);
    }
  };
  return (
    <IconButton
      icon={done() ? "check" : "copy"}
      label={done() ? "Đã chép" : (props.label ?? "Chép nội dung")}
      size="sm"
      onClick={() => void copy()}
    />
  );
}

/**
 * Đường dẫn dài: cắt ở *giữa*, giữ tên tệp — phần đuôi mới là phần phân biệt được.
 *
 * Khi có trình duyệt mã nguồn để mở vào thì đây là một cái nút: mọi đường dẫn trong bản
 * ghi đều là một chỗ người ta muốn nhìn vào, và bắt họ tự tìm lại tệp đó trong cây là
 * bắt họ làm cái việc mà bản ghi vừa nói cho họ biết.
 */
export function FilePath(props: { path: string; line?: number }) {
  const actions = useTranscriptActions();
  const open = () => actions.openFile;
  return (
    <Show
      when={open()}
      fallback={
        <span class="min-w-0 truncate font-mono text-xs text-accent-ink" dir="rtl" title={props.path}>
          <bdi>{props.path}</bdi>
        </span>
      }
    >
      {(go) => (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            go()(props.path, props.line);
          }}
          title={`Mở ${props.path}${props.line === undefined ? "" : ` ở dòng ${props.line}`}`}
          class="min-w-0 truncate rounded-btn font-mono text-xs text-accent-ink underline decoration-transparent underline-offset-2 transition-colors duration-[var(--dur-fast)] hover:decoration-current"
          dir="rtl"
        >
          <bdi>{props.path}</bdi>
        </button>
      )}
    </Show>
  );
}

/** Nhãn nhỏ đứng cạnh tiêu đề: mô hình, phạm vi, số đếm. */
export function Chip(props: { children: JSX.Element; tone?: "neutral" | "accent" | "warn" }) {
  return (
    <span
      class="inline-flex shrink-0 items-center gap-3xs rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
      classList={{
        "bg-[var(--overlay-faint)] text-muted": (props.tone ?? "neutral") === "neutral",
        "bg-accent-soft text-accent-ink": props.tone === "accent",
        "bg-warn-soft text-warn": props.tone === "warn",
      }}
    >
      {props.children}
    </span>
  );
}
