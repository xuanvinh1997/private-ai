import { createEffect, For, Show } from "solid-js";
import Icon, { type IconName } from "./Icon";

/** One row in the suggestion list. */
export interface Suggestion {
  /** The string inserted into the input. */
  value: string;
  /** Bold text at the start of the row; defaults to `value`. */
  label?: string;
  /** Leading icon; a familiar shape beside a command name is recognised faster than a hint read mid-typing. */
  icon?: IconName;
  /** Secondary text on the right. */
  hint?: string;
  /** Not selectable, with the reason given in `hint`. */
  disabled?: boolean;
}

/** Suggestion list floating over the composer, for both `@` and `/`. Focus stays in the input, so the list only
 * draws a cursor; that makes `aria-activedescendant` on the input mandatory or arrow keys announce nothing. */
export default function CompletionPopup(props: {
  items: Suggestion[];
  cursor: number;
  /** Id of the list element, for the input's `aria-controls`. */
  id: string;
  /** Builds a row id, for the input's `aria-activedescendant`. */
  optionId: (index: number) => string;
  onPick: (item: Suggestion) => void;
  onHover: (index: number) => void;
  /** Text shown when there are no suggestions; omitted, an empty list renders nothing. */
  empty?: string;
}) {
  let list: HTMLUListElement | undefined;

  // A keyboard cursor must drag the viewport with it, as in the session palette.
  createEffect(() => {
    const index = props.cursor;
    if (props.items.length === 0) return;
    list?.children[index]?.scrollIntoView({ block: "nearest" });
  });

  return (
    <Show when={props.items.length > 0 || props.empty !== undefined}>
      <div
        class="absolute bottom-full left-0 right-0 z-[var(--z-floating)] mb-2xs overflow-hidden rounded-panel border border-line bg-surface shadow-pop"
        // Clicking the list must not steal focus: losing it collapses the composer before the click lands.
        onMouseDown={(event) => event.preventDefault()}
      >
        <Show
          when={props.items.length > 0}
          fallback={<p class="m-0 px-md py-sm text-sm text-faint">{props.empty}</p>}
        >
          <ul
            ref={list}
            id={props.id}
            role="listbox"
            class="m-0 max-h-[240px] list-none overflow-y-auto p-2xs"
          >
            <For each={props.items}>
              {(item, index) => (
                <li role="presentation">
                  <button
                    type="button"
                    id={props.optionId(index())}
                    role="option"
                    aria-selected={index() === props.cursor}
                    aria-disabled={item.disabled === true}
                    onClick={() => {
                      if (item.disabled !== true) props.onPick(item);
                    }}
                    onMouseEnter={() => props.onHover(index())}
                    class="flex w-full items-center gap-xs rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink aria-[disabled=true]:opacity-50"
                  >
                    <Show when={item.icon}>
                      {(icon) => (
                        <span class="shrink-0 text-muted">
                          <Icon name={icon()} size={14} />
                        </span>
                      )}
                    </Show>
                    <span class="min-w-0 flex-1 truncate font-mono text-sm">
                      {item.label ?? item.value}
                    </span>
                    <Show when={item.hint}>
                      <span class="shrink-0 text-2xs text-faint">{item.hint}</span>
                    </Show>
                    <Show when={item.disabled !== true}>
                      <Icon name="enter" size={12} />
                    </Show>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </Show>
  );
}
