import { Show } from "solid-js";
import type { NodeProps } from "../../lib/registry";
import Icon from "../Icon";

/**
 * Thông báo ngoài câu trả lời.
 *
 * Thụt vào bằng đúng bề rộng avatar cộng khoảng cách của một tin nhắn, nên nó nằm trên
 * cùng một trục dọc với nội dung thật thay vì tạo ra một mép thứ hai.
 */
export function NoticeNode(props: NodeProps<"notice">) {
  return (
    <p
      class="m-0 flex items-center gap-sm pl-[calc(var(--avatar)+var(--sp-md))] text-xs text-faint"
      role="note"
    >
      <span class="h-px flex-none w-lg bg-line" aria-hidden="true" />
      {props.node.message}
    </p>
  );
}

export function ProgressNode(props: NodeProps<"progress">) {
  return (
    <p
      class="m-0 flex items-center gap-sm pl-[calc(var(--avatar)+var(--sp-md))] text-xs text-muted"
      role="status"
    >
      <span class="size-1.5 shrink-0 rounded-pill bg-accent motion-safe:animate-pulse" aria-hidden="true" />
      {props.node.label}
      <Show when={props.node.detail}>
        {(detail) => <span class="min-w-0 truncate font-mono text-2xs text-faint">{detail()}</span>}
      </Show>
    </p>
  );
}

/**
 * Lỗi cấp lượt.
 *
 * `role="alert"` chứ không `aria-live="polite"`: một lượt hỏng là thứ người dùng cần
 * biết ngay, không phải sau khi trình đọc màn hình nói hết câu đang dở.
 */
export function ErrorNode(props: NodeProps<"error">) {
  return (
    <div
      role="alert"
      class="ml-[calc(var(--avatar)+var(--sp-md))] flex items-start gap-sm rounded-panel border border-danger-soft bg-danger-soft px-md py-xs text-sm text-danger"
    >
      <span class="mt-3xs shrink-0">
        <Icon name="x" size={14} />
      </span>
      <span class="min-w-0">{props.node.message}</span>
    </div>
  );
}
