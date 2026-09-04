import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { S, t } from "../lib/i18n";
import { rankSessions, relativeTime } from "../lib/sessions";
import Icon from "./Icon";

import type { SessionSummary } from "../lib/protocol";

/** Session search palette: filter by name and open, nothing else. The ranking lives in [`rankSessions`], since it
 * is pure logic. Focus never leaves the input, so `aria-activedescendant` is what makes arrow keys audible. */
export default function SessionPalette(props: {
  sessions: SessionSummary[];
  /** The open session, so it can be marked in the list. */
  currentId?: string;
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  let panel: HTMLDivElement | undefined;
  let list: HTMLUListElement | undefined;
  const [query, setQuery] = createSignal("");
  const [cursor, setCursor] = createSignal(0);

  useFocusTrap(() => panel, props.onClose);

  const matches = createMemo(() => rankSessions(props.sessions, query()));

  const optionId = (index: number) => `palette-opt-${index}`;

  // A keyboard cursor must drag the viewport, or Enter opens a session the user cannot see; `nearest` scrolls minimally.
  createEffect(() => {
    const index = cursor();
    if (matches().length === 0) return;
    list?.children[index]?.scrollIntoView({ block: "nearest" });
  });

  const move = (delta: number) => {
    const count = matches().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  const jump = (to: number) => {
    if (matches().length > 0) setCursor(to);
  };

  const pick = () => {
    const chosen = matches()[cursor()];
    if (chosen) props.onPick(chosen.id);
  };

  return (
    <div
      class="fixed inset-0 z-[var(--z-modal)] flex items-start justify-center p-4xl"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label={t(S.chat.palette.title)}
        class="flex max-h-[60vh] w-full max-w-[520px] flex-col overflow-hidden rounded-card border border-line bg-surface shadow-pop"
      >
        <input
          type="text"
          role="combobox"
          value={query()}
          placeholder={t(S.chat.sessionSearch)}
          aria-label={t(S.chat.palette.field)}
          aria-controls="palette-results"
          aria-expanded={matches().length > 0}
          aria-autocomplete="list"
          aria-activedescendant={matches().length > 0 ? optionId(cursor()) : undefined}
          autocomplete="off"
          spellcheck={false}
          onInput={(event) => {
            setQuery(event.currentTarget.value);
            setCursor(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            } else if (event.key === "Home") {
              event.preventDefault();
              jump(0);
            } else if (event.key === "End") {
              event.preventDefault();
              jump(matches().length - 1);
            } else if (event.key === "Enter") {
              event.preventDefault();
              pick();
            }
          }}
          class="border-b border-line-strong bg-transparent px-(--dialog-pad-x) py-md text-base text-text outline-none transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        />
        <Show
          when={matches().length > 0}
          fallback={
            <p class="flex items-center gap-2xs px-(--dialog-pad-x) py-md text-sm text-faint">
              <Icon name="search" size={14} />
              {t(S.chat.noSessionMatch)}
            </p>
          }
        >
          <ul
            ref={list}
            id="palette-results"
            role="listbox"
            aria-label={t(S.chat.palette.results)}
            class="m-0 list-none overflow-y-auto p-2xs"
          >
            <For each={matches()}>
              {(session, index) => (
                <li role="presentation">
                  <button
                    type="button"
                    id={optionId(index())}
                    role="option"
                    onClick={() => props.onPick(session.id)}
                    onMouseEnter={() => setCursor(index())}
                    aria-selected={index() === cursor()}
                    class="flex w-full flex-col items-start gap-3xs rounded-btn px-md py-2xs text-left transition-colors hover:bg-surface-hover aria-[selected=true]:bg-accent-soft aria-[selected=true]:text-accent-ink"
                  >
                    <span class="flex w-full items-center gap-xs">
                      <span class="min-w-0 flex-1 truncate text-sm text-text">{session.title}</span>
                      {/* The open session, marked with a word rather than a coloured dot: a dot has to be guessed. */}
                      <Show when={session.id === props.currentId}>
                        <span class="shrink-0 text-2xs text-faint">
                          {t(S.chat.palette.current)}
                        </span>
                      </Show>
                    </span>
                    <span class="text-2xs text-faint">{relativeTime(session.updatedAt)}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </div>
  );
}
