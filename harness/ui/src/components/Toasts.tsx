import { For } from "solid-js";
import { S, t } from "../lib/i18n";
import { dismissToast, toasts } from "../lib/toast";
import Icon from "./Icon";
import { IconButton } from "./primitives";

/** Toast stack in the workspace's top-right, the only corner with nothing competing for space; below the top bar
 * so it never covers the drag region. `z-[60]` puts it above dialogs, which is usually where toasts come from.
 * The strip itself is `pointer-events-none`, since an invisible click-eating area is untraceable. */
export default function Toasts() {
  return (
    <div class="pointer-events-none fixed top-(--header-h) right-0 z-[60] flex w-[min(26rem,calc(100vw-2rem))] flex-col gap-2xs p-md">
      <For each={toasts()}>
        {(toast) => (
          <div
            // `alert` for errors, `status` for the rest: an error answers a gesture the user is waiting on.
            role={toast.kind === "error" ? "alert" : "status"}
            class="pointer-events-auto flex items-start gap-2xs rounded-card border border-line bg-surface py-sm pr-2xs pl-md shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
          >
            <span
              class="mt-3xs shrink-0"
              classList={{
                "text-warn": toast.kind === "error",
                "text-muted": toast.kind !== "error",
              }}
              aria-hidden="true"
            >
              <Icon name={toast.kind === "error" ? "warn" : "bubble"} size={14} />
            </span>

            {/* `break-words` rather than an ellipsis: errors carry filenames, and the tail is what distinguishes them. */}
            <p class="m-0 min-w-0 flex-1 py-3xs text-xs break-words text-text">{toast.text}</p>

            <IconButton
              icon="x"
              size="sm"
              label={t(S.chat.toast.close)}
              tip="left"
              onClick={() => dismissToast(toast.id)}
            />
          </div>
        )}
      </For>
    </div>
  );
}
