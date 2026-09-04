import { createMemo, createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import { S, t } from "../lib/i18n";
import type { ModelChoice } from "../lib/protocol";
import Icon from "./Icon";

/** Whether a model may appear in the chat picker; hide only the embedding-only ones, since filtering on
 * `chat === true` would erase a usable model whenever the core had to guess capabilities from a name. */
export const usableForChat = (choice: ModelChoice): boolean =>
  !(choice.embedding && !choice.chat);

/** Model picker, placed *inside* the composer: the model is a property of the message about to be sent, so it is
 * re-read before every click. Not the shared `Menu`, since each row carries two lines and two badges. */
export default function ModelPicker(props: {
  value: string;
  models: ModelChoice[];
  onPick: (id: string) => void;
  /** Open settings, model providers page. */
  onManageProviders: () => void;
  disabled?: boolean;
}) {
  const id = createUniqueId();
  const [open, setOpen] = createSignal(false);
  const choices = createMemo(() => props.models.filter(usableForChat));
  let popup: HTMLDivElement | undefined;
  let trigger: HTMLButtonElement | undefined;

  // Click outside closes; listening in the capture phase so the click cannot hit another button first.
  const onDocPointerDown = (event: PointerEvent) => {
    const target = event.target as Node | null;
    if (popup?.contains(target ?? null) || trigger?.contains(target ?? null)) return;
    setOpen(false);
  };
  document.addEventListener("pointerdown", onDocPointerDown, true);
  onCleanup(() => document.removeEventListener("pointerdown", onDocPointerDown, true));

  const move = (delta: number) => {
    const buttons = [...(popup?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    if (buttons.length === 0) return;
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + delta + buttons.length) % buttons.length]?.focus();
  };

  const close = (restore: boolean) => {
    setOpen(false);
    if (restore) trigger?.focus();
  };

  return (
    <div class="relative">
      <button
        ref={trigger}
        type="button"
        disabled={props.disabled}
        aria-haspopup="menu"
        aria-expanded={open()}
        aria-controls={id}
        aria-label={t(S.chat.model.trigger, { name: props.value })}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            queueMicrotask(() => move(1));
          }
        }}
        class="flex h-(--control-h) items-center gap-3xs rounded-pill border border-line bg-surface-soft px-sm text-xs text-muted shadow-control transition-colors duration-[var(--dur-fast)] disabled:opacity-40 enabled:hover:border-line-strong enabled:hover:bg-surface enabled:hover:text-ink"
      >
        <Icon name="model" size={13} />
        <span class="max-w-40 truncate">{props.value}</span>
        <Icon name="chevron-down" size={12} />
      </button>

      <Show when={open()}>
        <div
          ref={popup}
          id={id}
          role="menu"
          aria-label={t(S.common.model)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              close(true);
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              move(1);
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              move(-1);
            }
          }}
          // Open upward: the composer sits at the bottom, so a downward menu has nowhere to go.
          class="absolute bottom-full left-0 z-[var(--z-popover)] mb-3xs flex w-[min(22rem,72vw)] flex-col rounded-menu border border-line bg-surface p-3xs shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        >
          {/* Two reasons for an empty list, two messages: unreachable server versus a server with no chat model. */}
          <Show
            when={choices().length > 0}
            fallback={
              <p class="m-0 flex items-center gap-2xs px-sm py-xs text-2xs text-faint">
                <Icon name="model" size={13} />
                {t(
                  props.models.length === 0 ? S.chat.model.noServer : S.chat.model.embedOnly,
                )}
              </p>
            }
          >
            <ul class="m-0 flex max-h-72 list-none flex-col gap-3xs overflow-y-auto p-0">
              <For each={choices()}>
                {(choice) => (
                  <li>
                    <button
                      type="button"
                      role="menuitemradio"
                      aria-checked={choice.id === props.value}
                      onClick={() => {
                        close(true);
                        props.onPick(choice.id);
                      }}
                      class="flex w-full items-start gap-sm rounded-btn px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[checked=true]:bg-accent-soft"
                    >
                      <span
                        class="mt-3xs shrink-0"
                        classList={{
                          "text-accent-ink": choice.id === props.value,
                          "text-transparent": choice.id !== props.value,
                        }}
                      >
                        <Icon name="check" size={13} />
                      </span>
                      <span class="flex min-w-0 flex-1 flex-col gap-3xs">
                        <span class="min-w-0 truncate font-mono text-xs text-text">
                          {choice.id}
                        </span>
                        <span class="flex flex-wrap items-center gap-2xs text-2xs">
                          {/* Said here rather than after the choice: the cost of picking wrong is a silently useless assistant. */}
                          <Show
                            when={choice.tools}
                            fallback={
                              <span class="text-warn">{t(S.chat.model.noTools)}</span>
                            }
                          >
                            <span class="text-muted">{t(S.chat.model.hasTools)}</span>
                          </Show>
                          <Show when={choice.contextWindow}>
                            {(size) => (
                              <span class="text-faint tabular-nums">
                                {t(S.chat.model.context, { n: Math.round(size() / 1024) })}
                              </span>
                            )}
                          </Show>
                        </span>
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>

          <div class="mt-3xs border-t border-line pt-3xs">
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                close(true);
                props.onManageProviders();
              }}
              class="flex w-full items-center gap-sm rounded-btn px-sm py-2xs text-left text-xs text-text transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
            >
              <Icon name="server" size={14} />
              {t(S.chat.model.providers)}
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}
