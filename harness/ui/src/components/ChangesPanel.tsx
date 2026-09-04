import { createSignal, For, Show } from "solid-js";
import { baseName, dirName, type ChangedFile } from "../lib/changes";
import { S, t, tn } from "../lib/i18n";
import DiffBlock from "./DiffBlock";
import Icon from "./Icon";
import { IconButton } from "./primitives";

/** The "files changed this session" panel: one row per file, folded out of the transcript, acting as its index. */
export function ChangesPanelContent(props: {
  files: ChangedFile[];
  onReveal: (nodeId: string) => void;
}) {
  return (
    <div class="flex min-h-0 flex-1 flex-col">
      <Show
        when={props.files.length > 0}
        fallback={
          <p class="flex items-start gap-2xs px-md py-lg text-xs text-faint">
            <span class="mt-3xs shrink-0">
              <Icon name="diff" size={13} />
            </span>
            {t(S.chat.changes.empty)}
          </p>
        }
      >
        <Totals files={props.files} class="border-b border-line px-md py-xs" />

        <ul class="m-0 min-h-0 flex-1 list-none overflow-y-auto p-sm">
          <For each={props.files}>
            {(file) => (
              <li>
                <button
                  type="button"
                  onClick={() => props.onReveal(file.nodeId)}
                  class="flex w-full items-center gap-sm rounded-panel px-sm py-xs text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
                >
                  <span
                    class="shrink-0"
                    classList={{ "text-warn": file.pending, "text-muted": !file.pending }}
                  >
                    <Icon name="diff" size={15} />
                  </span>
                  <span class="flex min-w-0 flex-1 flex-col">
                    <span class="flex min-w-0 items-baseline gap-2xs">
                      <span class="min-w-0 truncate font-mono text-xs text-text">
                        {baseName(file.path)}
                      </span>
                      <Show when={file.created}>
                        <span class="shrink-0 text-2xs text-success">
                          {t(S.chat.changes.created)}
                        </span>
                      </Show>
                      <Show when={file.pending}>
                        <span class="shrink-0 text-2xs text-warn">
                          {t(S.chat.changes.pending)}
                        </span>
                      </Show>
                    </span>
                    <Show when={dirName(file.path)}>
                      {(dir) => (
                        <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={file.path}>
                          <bdi>{dir()}</bdi>
                        </span>
                      )}
                    </Show>
                  </span>
                  <Counts added={file.added} removed={file.removed} />
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}

/** Full-page changes view: unlike the side panel, a row expands its diff in place, and every row starts open. */
export function ChangesBoard(props: {
  files: ChangedFile[];
  onReveal: (nodeId: string) => void;
}) {
  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-(--reading-measure) flex-col gap-md">
        <Show
          when={props.files.length > 0}
          fallback={
            <p class="m-0 flex items-center gap-2xs text-sm text-faint">
              <Icon name="diff" size={14} />
              {t(S.chat.changes.empty)}
            </p>
          }
        >
          <Totals files={props.files} class="px-3xs" />
          <For each={props.files}>{(file) => <FileReview file={file} onReveal={props.onReveal} />}</For>
        </Show>
      </div>
    </div>
  );
}

function FileReview(props: { file: ChangedFile; onReveal: (nodeId: string) => void }) {
  const [open, setOpen] = createSignal(true);
  return (
    <section class="overflow-hidden rounded-card border border-line bg-surface">
      <div class="flex items-center gap-sm px-(--card-pad-x) py-(--card-pad-y)">
        {/* The whole filename strip is the toggle: the largest hit target should be the most common action. */}
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open()}
          class="flex min-w-0 flex-1 items-center gap-sm text-left"
        >
          <span
            class="shrink-0 text-muted transition-transform duration-[var(--dur-fast)]"
            classList={{ "rotate-90": open() }}
          >
            <Icon name="chevron-right" size={14} />
          </span>
          <span
            class="min-w-0 flex-1 truncate font-mono text-xs text-text"
            dir="rtl"
            title={props.file.path}
          >
            <bdi>{props.file.path}</bdi>
          </span>
          <Show when={props.file.created}>
            <span class="shrink-0 text-2xs text-success">{t(S.chat.changes.created)}</span>
          </Show>
          <Show when={props.file.pending}>
            <span class="shrink-0 text-2xs text-warn">{t(S.chat.changes.pending)}</span>
          </Show>
          <Counts added={props.file.added} removed={props.file.removed} />
        </button>

        {/* The route back to the transcript remains, but is no longer primary; it answers a different question. */}
        <IconButton
          icon="chat"
          size="sm"
          label={t(S.chat.changes.reveal, { name: baseName(props.file.path) })}
          onClick={() => props.onReveal(props.file.nodeId)}
        />
      </div>

      <Show when={open() && props.file.hunks.length > 0}>
        <div class="border-t border-line p-(--card-pad-y)">
          {/* A much higher line cap than in chat: here the diff *is* the content, not an excerpt. */}
          <DiffBlock diffs={props.file.hunks} maxLines={40} />
        </div>
      </Show>
    </section>
  );
}

/** Totals row: how many files, how many lines added, how many removed. */
function Totals(props: { files: ChangedFile[]; class?: string }) {
  const added = () => props.files.reduce((sum, file) => sum + file.added, 0);
  const removed = () => props.files.reduce((sum, file) => sum + file.removed, 0);
  return (
    <div class={`flex items-center gap-sm text-2xs tabular-nums ${props.class ?? ""}`}>
      <span class="text-muted">
        {tn(props.files.length, S.chat.changes.fileOne, S.chat.changes.fileMany)}
      </span>
      <span class="text-success">+{added()}</span>
      <span class="text-danger">−{removed()}</span>
    </div>
  );
}

function Counts(props: { added: number; removed: number }) {
  return (
    <span class="shrink-0 text-2xs tabular-nums">
      <span class="text-success">+{props.added}</span>{" "}
      <span class="text-danger">−{props.removed}</span>
    </span>
  );
}
