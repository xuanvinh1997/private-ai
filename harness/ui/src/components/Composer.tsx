import { createEffect, createMemo, createResource, createSignal, For, Show } from "solid-js";
import { useDragDrop } from "../hooks/useDragDrop";
import {
  type Attached,
  type Attachment,
  attached,
  pickFiles,
  resolveAttachments,
} from "../lib/attach";
import { applyCompletion, completePaths, findTrigger, rankCommands } from "../lib/complete";
import CompletionPopup, { type Suggestion } from "./CompletionPopup";
import { S, t, tn, type Msg } from "../lib/i18n";
import { displayMode } from "../lib/prefs";
import { notify } from "../lib/toast";
import type { ModelChoice, ProjectKind, ToolScope } from "../lib/protocol";
import Icon, { type IconName } from "./Icon";
import Menu from "./Menu";
import ModelPicker from "./ModelPicker";
import { IconButton } from "./primitives";

/** Icon per `/` command; kept here, not in `lib/complete.ts`, so the command list stays testable without the UI. */
const COMMAND_ICON: Record<string, IconName> = {
  moi: "plus",
  tim: "search",
  duan: "folder",
  thaydoi: "diff",
  taplieu: "library",
  mohinh: "model",
  mcp: "plug",
  quyen: "hand",
  phimtat: "enter",
  caidat: "settings",
};

const SCOPE_LABEL: Record<ToolScope, Msg> = {
  read: S.chat.composer.scopeRead,
  write: S.chat.composer.scopeWrite,
  shell: S.chat.composer.scopeShell,
};

/** One piece of the status line under the composer; local because a shared primitive with one caller is premature.
 * `note` is a quieter suffix rather than its own piece, and `warn` is only for a piece that is actually worrying. */
type MetaBit = { icon: IconName; text: string; note?: string; warn?: boolean };

/** The composer: one rounded block at the bottom with every control inside its border, controlled from outside so
 * a dropped path can reach the draft and the draft survives a session switch. Tool scope is never hidden behind a
 * menu, since an open permission must be readable without clicking, and it ships with the next turn rather than
 * being stored. Everything around the input sits in three tiers - actionable pills, read-only text, and
 * conditional warnings - which lowers noise without hiding anything. */
export default function Composer(props: {
  value: string;
  onChange: (text: string) => void;
  onSubmit: () => void;
  /** Hard-lock the input, only while switching projects; never for a running turn, which queues instead. */
  disabled: boolean;
  busy: boolean;
  /** Message waiting for the current turn to end; an empty string means nothing is queued. */
  queued?: string;
  /** Drop the queued message. */
  onUnqueue?: () => void;
  onStop: () => void;
  model: string;
  models: ModelChoice[];
  onPickModel: (model: string) => void;
  /** Open settings, model providers, from inside the model picker. */
  onManageProviders: () => void;
  /** Warning under the model picker; `undefined` when there is nothing to say. */
  modelWarning?: string;
  scope: ToolScope;
  onPickScope: (scope: ToolScope) => void;
  /** Whether a project is open; without one no project tools are plugged in, so the scope picker must say so
   * rather than looking enabled, which is the worst lie a permissions UI can tell. */
  hasProject: boolean;
  /** The conversation an attachment belongs to: a file from outside the project is copied into this session's
   * own folder, and goes away with it. */
  sessionId: string;
  /** Files attached to the message being written, shown as chips above the field. Owned above the composer,
   * because the message that carries them is assembled there. */
  attachments: Attached[];
  onAttachmentsChange: (files: Attached[]) => void;
  /** Name of the open project, for the status line under the composer. */
  projectName?: string;
  projectKind?: ProjectKind;
  /** Number of *connected* MCP servers, not declared ones; `0` is never spelled out (see `meta` below). */
  mcpConnected: number;
  /** Something else sits below the composer (the prompt chips), so the bottom padding drops a step. */
  moreBelow?: boolean;
  /** Run a `/` command; omitted, the command palette never opens. */
  onCommand?: (name: string) => void;
  /** Context used by the latest step; a `null` `window` means no denominator, so only the token count is shown. */
  usage?: { used: number; window: number | null } | null;
}) {
  let composing = false;
  let field: HTMLTextAreaElement | undefined;
  const [focused, setFocused] = createSignal(false);

  // ---- `@` and `/` completion -----------------------------------------------
  //
  // The caret is tracked here rather than read from `field.selectionStart` during render, which reads the DOM
  // mid-build and never updates when the caret moves without the text changing.
  const [caret, setCaret] = createSignal(0);
  const [dismissed, setDismissed] = createSignal(false);
  const [cursor, setCursor] = createSignal(0);

  const trigger = createMemo(() => {
    if (dismissed()) return null;
    const found = findTrigger(props.value, caret());
    if (found?.kind === "command" && props.onCommand === undefined) return null;
    return found;
  });

  // Only ask the core while a path is actually being typed; `createResource` drops the previous call per keystroke.
  const [paths] = createResource(
    () => (trigger()?.kind === "path" ? trigger()!.query : null),
    (query) => completePaths(query, 8),
  );

  const items = createMemo<Suggestion[]>(() => {
    const found = trigger();
    if (!found) return [];
    if (found.kind === "command") {
      return rankCommands(found.query).map((command) => ({
        value: command.name,
        label: `/${command.name}`,
        icon: COMMAND_ICON[command.name] ?? "terminal",
        hint:
          command.needsProject === true && !props.hasProject
            ? t(S.chat.composer.needsProject)
            : command.hint,
        disabled: command.needsProject === true && !props.hasProject,
      }));
    }
    return (paths() ?? []).map((path) => ({ value: path }));
  });

  // A changed query resets the cursor: keeping the index would point at a different row and Enter would insert it.
  createEffect(() => {
    items();
    setCursor(0);
  });

  const open = () => trigger() !== null && items().length > 0;

  const moveCursor = (delta: number) => {
    const count = items().length;
    if (count === 0) return;
    setCursor((current) => (current + delta + count) % count);
  };

  /** Record the caret position after the browser has moved it. */
  const syncCaret = (el: HTMLTextAreaElement) => setCaret(el.selectionStart ?? 0);

  const choose = (item: Suggestion) => {
    const found = trigger();
    if (!found || item.disabled === true) return;
    if (found.kind === "command") {
      props.onChange("");
      setDismissed(true);
      props.onCommand?.(item.value);
      field?.focus();
      return;
    }
    const next = applyCompletion(props.value, found, item.value);
    props.onChange(next.text);
    // Restore the caret *after* Solid writes the new value, or the browser pushes it to the end of the string.
    queueMicrotask(() => {
      if (!field) return;
      field.setSelectionRange(next.caret, next.caret);
      setCaret(next.caret);
      field.focus();
    });
  };

  const optionId = (index: number) => `composer-opt-${index}`;

  /** Context fill ratio, or `null` when it is not yet worth saying. */
  const contextPressure = createMemo(() => {
    const counted = props.usage;
    if (!counted || counted.window === null || counted.window <= 0) return null;
    const ratio = counted.used / counted.window;
    return ratio >= 0.6 ? { ratio: Math.min(ratio, 1) } : null;
  });

  /** Status line under the composer: what the next turn will run with. Built as an array so the separator only
   * appears *between* real pieces, and pieces that only matter when non-default are absent at their default. */
  const meta = createMemo<MetaBit[]>(() => {
    const rows: MetaBit[] = [];

    if (props.hasProject) {
      rows.push({
        icon: "folder-open",
        text: props.projectName ?? t(S.common.project),
        note: t(
          props.projectKind === "docs" ? S.chat.composer.kindDocs : S.chat.composer.kindCode,
        ),
      });
    }

    // A count, not names: this line only answers "are there extra tools"; the MCP page answers which.
    if (props.mcpConnected > 0) {
      rows.push({
        icon: "plug",
        text: tn(props.mcpConnected, S.chat.composer.mcpOne, S.chat.composer.mcpMany),
      });
    }

    // Context pressure, shown only once it is worth worrying about: below 60% the number changes no decision, and
    // a permanent number trains the eye to skip the spot it will later need. The denominator is the compaction
    // plugin's threshold window, not the model's, so the warning lands just before compaction runs.
    const pressure = contextPressure();
    if (pressure) {
      rows.push({
        icon: "model",
        text: t(S.chat.composer.context, { n: Math.round(pressure.ratio * 100) }),
        warn: pressure.ratio >= 0.85,
      });
    }

    return rows;
  });

  /** Turn placed paths into chips. A rejected file never blocks the rest of the batch, and the error names
   * exactly the one that failed. A file from outside the project comes back as the core's copy of it, so the
   * path kept is not always the path dropped. */
  const attach = async (paths: string[]) => {
    if (paths.length === 0) return;

    let resolved: Attachment[];
    try {
      resolved = await resolveAttachments(paths, props.sessionId);
    } catch (err) {
      // The core rejected the whole batch, almost always "no project"; quote it verbatim, since only it knows why.
      notify("error", String(err));
      return;
    }

    const usable = resolved.filter((entry) => entry.error === null);
    const refused = resolved.filter((entry) => entry.error !== null);
    // One notice per batch, not per file; the first sentence is the specific one, naming a file, then the count.
    if (refused.length > 0) {
      notify(
        "error",
        refused.length === 1
          ? refused[0]!.error!
          : t(S.chat.composer.attachRefusedMore, {
              err: refused[0]!.error!,
              n: refused.length - 1,
            }),
      );
    }

    if (usable.length === 0) return;
    // The same file twice is one chip: dropping a batch that overlaps the last one is ordinary, and two
    // identical chips would send the model the same path twice.
    const co = new Set(props.attachments.map((file) => file.path));
    const them = usable.filter((entry) => !co.has(entry.path)).map(attached);
    if (them.length > 0) props.onAttachmentsChange([...props.attachments, ...them]);
    field?.focus();
  };

  const detach = (path: string) => {
    props.onAttachmentsChange(props.attachments.filter((file) => file.path !== path));
    field?.focus();
  };

  /** Drag and drop takes the same path as the attach button; `useDragDrop` broadcasts, so a drop must have exactly
   * one owner per screen, and in the conversation that owner is the composer. */
  useDragDrop((paths) => void attach(paths));

  /** The second entrance to the same job, the OS dialog; every exit path says something except an explicit cancel. */
  const browse = async () => {
    // With no project, answer immediately rather than opening a dialog only to refuse what they picked.
    if (!props.hasProject) {
      notify("error", t(S.chat.composer.attachNoProject));
      return;
    }
    try {
      const picked = await pickFiles();
      if (picked === null) {
        notify("error", t(S.chat.composer.attachNoPicker));
        return;
      }
      await attach(picked);
    } catch (err) {
      notify("error", t(S.chat.composer.attachPickerFailed, { err: String(err) }));
    }
  };

  // Not blocked while `busy`: App queues the message, and blocking here would make Enter do nothing mid-turn.
  // A message of attachments and no words is a real message -- "read this" is what the chips already say.
  const submit = () => {
    if (props.disabled) return;
    if (props.value.trim() === "" && props.attachments.length === 0) return;
    props.onSubmit();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    // A Vietnamese IME sends Enter to commit a word; without this guard every commit would send the message.
    if (composing || event.isComposing) return;

    // An open suggestion list claims the navigation keys first: Enter there inserts a suggestion, it does not send.
    if (open()) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        moveCursor(1);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        moveCursor(-1);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        const item = items()[cursor()];
        if (item && item.disabled !== true) {
          event.preventDefault();
          choose(item);
          return;
        }
      }
      if (event.key === "Escape") {
        event.preventDefault();
        // Close the list but keep the text; Esc everywhere else in the app closes without deleting.
        setDismissed(true);
        return;
      }
    }

    const chord = event.metaKey || event.ctrlKey;
    if (event.key === "Enter" && (chord || !event.shiftKey)) {
      event.preventDefault();
      submit();
    }
  };

  // Auto-height up to ~10 lines, measured via `scrollHeight` after resetting to 0, or it can never shrink again.
  const resize = (el: HTMLTextAreaElement) => {
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  return (
    <form
      class="shrink-0 bg-bg px-(--page-pad-x) pt-sm"
      classList={{
        "pb-(--page-pad-y)": props.moreBelow !== true,
        "pb-sm": props.moreBelow === true,
      }}
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      {/* The queued message, shown above the composer at the same width: an invisible queue makes Enter feel
          like a lost click. It carries a cancel button, since the streaming answer may already have answered it. */}
      <Show when={(props.queued ?? "") !== ""}>
        <div
          class="mx-auto mb-xs flex w-full items-center gap-xs rounded-panel border border-line bg-surface-soft px-md py-xs"
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <Icon name="clock" size={13} />
          <span class="shrink-0 text-xs text-faint">{t(S.chat.composer.queued)}</span>
          <span class="min-w-0 flex-1 truncate text-sm text-text">{props.queued}</span>
          {/* A fixed 28px height: this is the only way out of the queue, and a two-word target gets missed. */}
          <button
            type="button"
            onClick={() => props.onUnqueue?.()}
            aria-label={t(S.chat.composer.unqueue)}
            class="flex h-7 shrink-0 items-center rounded-btn px-2xs text-xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-text"
          >
            {t(S.common.remove)}
          </button>
        </div>
      </Show>

      <div
        // The composer matches the width of the text column above it, or the two read as unrelated blocks.
        // On focus the border takes the accent colour *and* a thin halo blooms: a one-pixel colour change is
        // invisible to peripheral vision, and this input sits at the bottom of a text-filled window. `transition`
        // rather than `transition-colors`, so the shadow and the border move together.
        class="relative mx-auto flex w-full flex-col rounded-composer border bg-surface shadow-float transition duration-[var(--dur-base)] ease-[var(--ease-out)]"
        classList={{
          "border-accent ring-[3px] ring-accent/15": focused(),
          "border-line-strong ring-0 ring-transparent": !focused(),
          "max-w-(--reading-measure)": displayMode() === "bubble",
          "max-w-[min(100%,980px)]": displayMode() === "document",
        }}
      >
        <CompletionPopup
          items={items()}
          cursor={cursor()}
          id="composer-completions"
          optionId={optionId}
          onPick={choose}
          onHover={setCursor}
          empty={
            trigger()?.kind === "path" && !paths.loading
              ? t(S.chat.composer.noPathMatch)
              : undefined
          }
        />

        {/* Chips, above the field: what is attached must be visible as objects that can be removed one by one.
            They used to be lines of text in the draft, which meant reading a path to know what was attached and
            editing text to drop one. The path itself moves to the `title`, where it is available but not loud. */}
        <Show when={props.attachments.length > 0}>
          <ul class="m-0 flex list-none flex-wrap gap-2xs px-md pt-md pb-0">
            <For each={props.attachments}>
              {(file) => (
                <li
                  title={file.path}
                  class="flex max-w-full items-center gap-2xs rounded-btn border border-line bg-[var(--overlay-hover)] py-3xs pr-3xs pl-2xs text-xs text-text"
                >
                  <Icon name={file.extracted ? "document" : "paperclip"} size={12} />
                  <span class="min-w-0 max-w-[220px] truncate">{file.name}</span>
                  <Show when={file.extracted}>
                    <span class="shrink-0 text-faint">{t(S.chat.composer.attachedExtracted)}</span>
                  </Show>
                  <button
                    type="button"
                    onClick={() => detach(file.path)}
                    aria-label={t(S.chat.composer.attachedRemove, { name: file.name })}
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-btn text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-text"
                  >
                    <Icon name="x" size={12} />
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <textarea
          ref={(el) => {
            field = el;
            queueMicrotask(() => resize(el));
          }}
          rows={1}
          value={props.value}
          disabled={props.disabled}
          placeholder={t(
            props.busy ? S.chat.composer.placeholderBusy : S.chat.composer.placeholder,
          )}
          aria-label={t(S.chat.composer.field)}
          aria-keyshortcuts="Enter Meta+Enter Control+Enter"
          onCompositionStart={() => (composing = true)}
          onCompositionEnd={() => (composing = false)}
          role="combobox"
          aria-expanded={open()}
          aria-controls="composer-completions"
          aria-activedescendant={open() ? optionId(cursor()) : undefined}
          aria-autocomplete="list"
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onInput={(event) => {
            props.onChange(event.currentTarget.value);
            resize(event.currentTarget);
            // Typing after Esc starts a new trigger, or one Esc would disable completion for the rest of the line.
            setDismissed(false);
            syncCaret(event.currentTarget);
          }}
          onKeyUp={(event) => syncCaret(event.currentTarget)}
          onClick={(event) => syncCaret(event.currentTarget)}
          onKeyDown={onKeyDown}
          class="max-h-[220px] w-full resize-none bg-transparent px-md pt-md pb-2xs text-base text-text outline-none placeholder:text-faint"
        />

        {/* Tier three: every conditional line, gathered into one wrapping row rather than three stacked strips,
            so toggling a condition does not shunt the button row up and down. `role="status"`, not `alert`,
            because these are standing conditions. No error text belongs here by rule: a failed attach describes
            something that just happened, so it goes to a toast (`lib/toast.ts`) instead. */}
        <Show when={!props.hasProject || props.modelWarning}>
          <div class="flex flex-wrap items-center gap-x-md gap-y-3xs px-md pb-2xs text-xs">
            {/* The sentence ends on "you can still send": a standing limit, not a breakage, and this is now the
                only place that says "no project" in words, together with its consequence. */}
            <Show when={!props.hasProject}>
              <p class="m-0 flex items-center gap-2xs text-muted" role="status">
                <Icon name="tools" size={12} />
                {t(S.chat.composer.noProject)}
              </p>
            </Show>

            <Show when={props.modelWarning}>
              {(message) => (
                <p class="m-0 flex items-center gap-2xs text-warn" role="status">
                  <Icon name="warn" size={12} />
                  {message()}
                </p>
              )}
            </Show>
          </div>
        </Show>

        {/* Tier one, the only row still wearing pills: attach, then scope, then model beside Send. Left to right
            reads "what goes in, what may be done with it, who does it", and all four change the next turn. */}
        <div class="flex flex-wrap items-center gap-2xs px-2xs pb-2xs">
          {/* This button opens the OS file dialog: Tauri's dialog *is* the OS layer and returns absolute paths;
              it is `<input type="file">` that cannot. Drag and drop stays as a shortcut, mentioned in the label.
              It is never disabled without a project: a grey button cannot say why it is grey, so it stays
              clickable and answers in words above itself. */}
          <IconButton
            icon="paperclip"
            label={t(S.chat.composer.attach)}
            onClick={() => void browse()}
          />

          {/* Disabled, not hidden, so the picker keeps its place and the change is visible; it also leaves the tab
              order, since there is nothing to choose. Said in words rather than struck through, because this is a
              permission with nothing to grant, not a broken one. `text-xs` matches the real pill pixel for pixel. */}
          <Show
            when={props.hasProject}
            fallback={
              <span
                aria-hidden="true"
                class="flex h-(--control-h) items-center gap-3xs rounded-pill border border-line bg-surface-soft px-sm text-xs text-faint shadow-control"
              >
                <Icon name="hand" size={13} />
                {t(SCOPE_LABEL[props.scope])}
                <span class="opacity-70">{t(S.chat.composer.scopeIdle)}</span>
              </span>
            }
          >
            {/* A hand rather than a wrench: the wrench says "there are tools", this row says how far they may go. */}
            <Menu
              variant="pill"
              placement="up"
              align="left"
              icon="hand"
              text={t(SCOPE_LABEL[props.scope])}
              tone={props.scope === "shell" ? "warn" : "neutral"}
              label={t(S.chat.composer.scopeMenu, { scope: t(SCOPE_LABEL[props.scope]) })}
              items={(["read", "write", "shell"] as ToolScope[]).map((scope) => ({
                id: scope,
                label: t(SCOPE_LABEL[scope]),
                icon: "hand" as const,
                onSelect: () => props.onPickScope(scope),
              }))}
            />
          </Show>

          <span class="flex-1" />

          <ModelPicker
            value={props.model}
            models={props.models}
            onPick={props.onPickModel}
            onManageProviders={props.onManageProviders}
            disabled={props.disabled || props.busy}
          />

          <Show
            when={props.busy}
            fallback={
              <button
                type="submit"
                aria-label={t(S.chat.composer.send)}
                disabled={
                  props.disabled ||
                  (props.value.trim() === "" && props.attachments.length === 0)
                }
                class="pai-btn pai-btn-primary"
              >
                <Icon name="send" size={14} />
              </button>
            }
          >
            <button
              type="button"
              aria-label={t(S.chat.composer.stop)}
              onClick={props.onStop}
              class="pai-btn pai-btn-danger-quiet"
            >
              <Icon name="stop" size={14} />
            </button>
          </Show>
        </div>
      </div>

      {/* Tier two, the status line: what the next turn will run with. Outside the border, because everything
          inside it is clickable while these pieces only report choices made elsewhere - so they are plain
          `--muted` text with small icons, not pills. `text-xs`, not smaller: this is the only line naming the
          directory the assistant is about to read. The model is deliberately absent, since its picker is inches
          above and this line exists to answer what the picker cannot. */}
      <Show when={meta().length > 0}>
        <div
          role="group"
          aria-label={t(S.chat.composer.metaLabel)}
          class="mx-auto mt-xs flex w-full flex-wrap items-center gap-x-2xs gap-y-3xs px-md text-xs text-muted"
          classList={{
            "max-w-(--reading-measure)": displayMode() === "bubble",
            "max-w-[min(100%,980px)]": displayMode() === "document",
          }}
        >
          <For each={meta()}>
            {(item, index) => (
              <span
                class="inline-flex items-center gap-2xs"
                classList={{ "text-warn": item.warn === true }}
              >
                {/* The separator is `aria-hidden`: the eye needs it, a screen reader already pauses between
                    elements. It lives *inside* the piece so `flex-wrap` never strands it at the end of a line,
                    and it stays `--faint` even on a warning piece, since the separator is not what warns. */}
                <Show when={index() > 0}>
                  <span aria-hidden="true" class="text-faint">
                    ·
                  </span>
                </Show>
                <Icon name={item.icon} size={12} />
                {item.text}
                <Show when={item.note}>
                  {(note) => <span class="text-faint">{note()}</span>}
                </Show>
              </span>
            )}
          </For>
        </div>
      </Show>
    </form>
  );
}
