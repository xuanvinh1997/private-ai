import { Show } from "solid-js";
import { S, tn } from "../../lib/i18n";
import type { TodoItem, ToolCall } from "../../lib/protocol";
import TodoCard from "../TodoCard";
import { ToolShell } from "./ToolCard";

/** Read the todo list out of raw arguments; a bad shape returns empty rather than throwing. */
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

/** `todo_write` draws the list itself, as the snapshot at call time; the `todo` node holds the latest state. */
export default function TodoToolCard(props: { call: ToolCall }) {
  const items = () => todosFromArgs(props.call.args);
  return (
    <ToolShell
      call={props.call}
      summary={tn(items().length, S.tools.todo.oneTask, S.tools.todo.manyTasks)}
    >
      <Show when={items().length > 0}>
        <TodoCard items={items()} />
      </Show>
    </ToolShell>
  );
}
