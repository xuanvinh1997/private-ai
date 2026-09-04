import { For, Show } from "solid-js";
import { S, t, type Msg } from "../lib/i18n";
import type { TodoItem, TodoStatus } from "../lib/protocol";

const LABEL: Record<TodoStatus, Msg> = {
  pending: S.chat.todo.statusPending,
  in_progress: S.chat.todo.statusRunning,
  done: S.chat.todo.statusDone,
  cancelled: S.chat.todo.statusCancelled,
};

/** The glyph is visual only; the meaning travels through the `aria-label` below. */
const MARK: Record<TodoStatus, string> = {
  pending: "○",
  in_progress: "◐",
  done: "●",
  cancelled: "×",
};

/** Todo list: a projection, not a timeline, so the card keeps no state of its own and shows only the latest send. */
export default function TodoCard(props: { items: TodoItem[] }) {
  const done = () => props.items.filter((item) => item.status === "done").length;
  return (
    <section
      class="flex flex-col gap-2xs rounded-panel border border-line bg-surface-soft px-md py-sm"
      aria-label={t(S.chat.todo.title)}
    >
      <header class="flex items-baseline justify-between gap-sm">
        <h3 class="m-0 text-xs font-medium text-ink">{t(S.chat.todo.title)}</h3>
        <span class="tabular-nums text-2xs text-faint">
          {done()}/{props.items.length}
        </span>
      </header>
      <Show
        when={props.items.length > 0}
        fallback={<p class="text-xs text-faint">{t(S.chat.todo.empty)}</p>}
      >
        <ul class="flex flex-col gap-3xs">
          <For each={props.items}>
            {(item) => (
              <li class="flex items-start gap-sm text-xs">
                <span
                  class="mt-3xs shrink-0"
                  role="img"
                  aria-label={t(LABEL[item.status])}
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
