import { For, Show, type JSX } from "solid-js";
import { displayMode } from "../lib/prefs";
import { clockTime } from "../lib/sessions";
import Icon, { type IconName } from "./Icon";
import { IconButton } from "./primitives";

export interface MessageAction {
  id: string;
  label: string;
  icon: IconName;
  danger?: boolean;
  onSelect: () => void;
}

/** Shared message frame: avatar, name and time, then content, then the action bar. Bubble mode right-aligns the
 * user's message without an avatar; document mode is full width. Assistant replies are never boxed, so a run of
 * them reads as one explanation. Actions appear on hover *or* keyboard focus within, never hover alone. */
export default function MessageShell(props: {
  role: "user" | "assistant";
  name: string;
  at: number;
  actions?: MessageAction[];
  live?: boolean;
  busy?: boolean;
  children: JSX.Element;
}) {
  const bubble = () => displayMode() === "bubble";
  const mine = () => props.role === "user";
  const flip = () => bubble() && mine();

  return (
    <article
      class="group flex gap-md"
      classList={{ "flex-row-reverse": flip() }}
      aria-live={props.live ? "polite" : undefined}
      aria-busy={props.busy || undefined}
    >
      <Show when={!flip()}>
        <div
          aria-hidden="true"
          class="mt-3xs grid size-(--avatar) shrink-0 place-items-center rounded-pill"
          classList={{
            "bg-accent text-on-accent": mine(),
            "bg-surface-hover text-accent-ink": !mine(),
          }}
        >
          <Icon name={mine() ? "chat" : "sparkle"} size={15} />
        </div>
      </Show>

      <div class="flex min-w-0 flex-1 flex-col gap-2xs" classList={{ "items-end": flip() }}>
        <div class="flex items-baseline gap-sm text-2xs">
          <span class="font-medium text-muted">{props.name}</span>
          <time class="text-faint tabular-nums">{clockTime(props.at)}</time>
        </div>

        <div
          class="min-w-0 max-w-full"
          classList={{
            "rounded-bubble bg-accent px-(--card-pad-x) py-(--card-pad-y) text-on-accent":
              bubble() && mine(),
            // Assistant: no border, background or padding; the text flows straight on the page.
            "w-full": !bubble() || !mine(),
          }}
        >
          {props.children}
        </div>

        <Show when={props.actions && props.actions.length > 0}>
          <div
            class="flex items-center gap-3xs opacity-0 transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
            classList={{ "flex-row-reverse": flip() }}
          >
            <For each={props.actions}>
              {(action) => (
                <IconButton
                  icon={action.icon}
                  label={action.label}
                  size="sm"
                  danger={action.danger}
                  onClick={action.onSelect}
                />
              )}
            </For>
          </div>
        </Show>
      </div>
    </article>
  );
}
