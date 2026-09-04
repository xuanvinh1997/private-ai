import { createMemo, createSignal, For, Show } from "solid-js";
import { diffRows, diffToText, diffTotals, foldRows } from "../lib/diff";
import { S, t, tn } from "../lib/i18n";
import type { DiffHunk } from "../lib/protocol";
import { CopyButton } from "./primitives";

/** Inline in chat, eight lines is enough to recognise a change without swallowing the screen. */
const CHAT_MAX_LINES = 8;

/** Stacked diff block: added and removed lines differ by background, not `+`/`-`, so selected text pastes as code.
 * Colour alone is not enough, so each line also carries an added/removed `aria-label`. */
export default function DiffBlock(props: { diffs: DiffHunk[]; maxLines?: number }) {
  const [expanded, setExpanded] = createSignal(false);
  const all = createMemo(() => diffRows(props.diffs));
  const limit = () => props.maxLines ?? CHAT_MAX_LINES;
  const shown = createMemo(() => (expanded() ? all() : foldRows(all(), limit())));
  const totals = createMemo(() => diffTotals(props.diffs));
  const foldable = () => all().length > limit();

  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="text-2xs text-muted">
          {tn(totals().files, S.chat.diff.captionOne, S.chat.diff.captionMany)}
        </figcaption>
        <div class="flex items-center gap-3xs">
          <Show when={foldable()}>
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              aria-expanded={expanded()}
              class="rounded-btn px-2xs py-3xs text-2xs text-muted transition-colors hover:bg-surface-hover hover:text-text"
            >
              {expanded()
                ? t(S.chat.diff.collapse)
                : t(S.chat.diff.expand, { n: all().length })}
            </button>
          </Show>
          <CopyButton text={() => diffToText(props.diffs)} label={t(S.chat.diff.copy)} />
        </div>
      </div>

      {/* Horizontal scrolling is contained here: a long code line must not stretch the page. */}
      <div class="overflow-x-auto">
        <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
          <For each={shown()}>
            {(row) => (
              <div
                class="flex items-start gap-sm px-sm"
                classList={{
                  "bg-surface-soft text-muted": row.kind === "path",
                  "bg-danger-soft text-text": row.kind === "del",
                  "bg-success-soft text-text": row.kind === "add",
                  "text-faint italic": row.kind === "gap",
                }}
              >
                <span
                  aria-hidden="true"
                  class="w-8 shrink-0 text-right text-faint tabular-nums select-none"
                >
                  {row.oldNo ?? row.newNo ?? ""}
                </span>
                <span
                  class="whitespace-pre"
                  aria-label={
                    row.kind === "add"
                      ? t(S.chat.diff.lineAdded, { text: row.text })
                      : row.kind === "del"
                        ? t(S.chat.diff.lineRemoved, { text: row.text })
                        : undefined
                  }
                >
                  {row.text === "" ? " " : row.text}
                </span>
              </div>
            )}
          </For>
        </div>
      </div>

      <div class="border-t border-line px-sm py-3xs text-2xs text-faint tabular-nums">
        └ <span class="text-success">+{totals().added}</span>{" "}
        <span class="text-danger">−{totals().removed}</span> ·{" "}
        {tn(totals().files, S.chat.changes.fileOne, S.chat.changes.fileMany)}
      </div>
    </figure>
  );
}
