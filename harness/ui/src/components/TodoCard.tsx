import { For, Show } from "solid-js";
import type { TodoItem, TodoStatus } from "../lib/protocol";

const LABEL: Record<TodoStatus, string> = {
  pending: "chưa làm",
  in_progress: "đang làm",
  done: "xong",
  cancelled: "bỏ",
};

/** Ký hiệu chỉ là phần thị giác; ý nghĩa đi qua `aria-label` bên dưới. */
const MARK: Record<TodoStatus, string> = {
  pending: "○",
  in_progress: "◐",
  done: "●",
  cancelled: "×",
};

/**
 * Danh sách việc.
 *
 * Đây là *projection*, không phải dòng thời gian: mỗi lần lõi gửi là toàn bộ danh sách
 * mới, và giao diện ghi đè tại chỗ. Vì thế thẻ này không giữ trạng thái nào của riêng
 * nó — mọi thứ nhìn thấy đều đến từ lần gửi gần nhất.
 */
export default function TodoCard(props: { items: TodoItem[] }) {
  const done = () => props.items.filter((item) => item.status === "done").length;
  return (
    <section
      class="flex flex-col gap-2xs rounded-panel border border-line bg-surface-soft px-md py-sm"
      aria-label="Danh sách việc"
    >
      <header class="flex items-baseline justify-between gap-sm">
        <h3 class="m-0 text-xs font-medium text-ink">Danh sách việc</h3>
        <span class="tabular-nums text-2xs text-faint">
          {done()}/{props.items.length}
        </span>
      </header>
      <Show
        when={props.items.length > 0}
        fallback={<p class="text-xs text-faint">Chưa có việc nào.</p>}
      >
        <ul class="flex flex-col gap-3xs">
          <For each={props.items}>
            {(item) => (
              <li class="flex items-start gap-sm text-xs">
                <span
                  class="mt-3xs shrink-0"
                  role="img"
                  aria-label={LABEL[item.status]}
                  classList={{
                    "text-faint": item.status === "pending",
                    "text-warn": item.status === "in_progress",
                    "text-success": item.status === "done",
                    "text-danger": item.status === "cancelled",
                  }}
                >
                  {MARK[item.status]}
                </span>
                <span
                  class="min-w-0"
                  classList={{
                    "text-text": item.status !== "cancelled" && item.status !== "done",
                    "text-muted": item.status === "done",
                    "text-faint line-through": item.status === "cancelled",
                  }}
                >
                  {item.text}
                </span>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}
