import { Show } from "solid-js";
import type { TodoItem, ToolCall } from "../../lib/protocol";
import TodoCard from "../TodoCard";
import { ToolShell } from "./ToolCard";

/** Đọc danh sách việc từ đối số thô. Sai hình dạng thì trả rỗng, không ném. */
function todosFromArgs(args: unknown): TodoItem[] {
  if (args === null || typeof args !== "object") return [];
  const raw = (args as Record<string, unknown>).todos;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((entry, index) => {
    if (entry === null || typeof entry !== "object") return [];
    const bag = entry as Record<string, unknown>;
    const text = typeof bag.text === "string" ? bag.text : typeof bag.content === "string" ? bag.content : null;
    if (text === null) return [];
    const status = bag.status;
    return [
      {
        id: typeof bag.id === "string" ? bag.id : `todo-${index}`,
        text,
        status:
          status === "in_progress" || status === "done" || status === "cancelled"
            ? status
            : "pending",
      } satisfies TodoItem,
    ];
  });
}

/**
 * `todo_write` vẽ luôn danh sách kết quả thay vì JSON đối số.
 *
 * Node `todo` riêng vẫn tồn tại và mang trạng thái mới nhất; thẻ này chỉ nói "lượt này
 * đã đụng vào danh sách", nên nó hiện *ảnh chụp lúc gọi*, không đồng bộ ngược.
 */
export default function TodoToolCard(props: { call: ToolCall }) {
  const items = () => todosFromArgs(props.call.args);
  return (
    <ToolShell call={props.call} summary={`${items().length} việc`}>
      <Show when={items().length > 0}>
        <TodoCard items={items()} />
      </Show>
    </ToolShell>
  );
}
