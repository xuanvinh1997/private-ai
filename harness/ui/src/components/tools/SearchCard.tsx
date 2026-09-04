import { For, Show } from "solid-js";
import { S, t, tn } from "../../lib/i18n";
import type { ToolCall } from "../../lib/protocol";
import { useTranscriptActions } from "../../lib/transcriptActions";
import { Disclosure, FilePath } from "../primitives";
import { ToolShell } from "./ToolCard";

/** How many matches show without expanding: enough to tell whether the search went the right way. */
const PEEK = 3;

function pattern(call: ToolCall): string {
  const bag = call.args as Record<string, unknown> | null;
  if (bag === null || typeof bag !== "object") return "";
  const raw = bag.pattern ?? bag.query ?? bag.glob;
  return typeof raw === "string" ? raw : "";
}

/** The `grep` card, grouped by file; `truncated` is stated, since "no more" and "stopped counting" differ. */
export function GrepCard(props: { call: ToolCall }) {
  const search = () => props.call.meta?.search;
  const groups = () => search()?.groups ?? [];

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">{pattern(props.call)}</code>
          <Show when={search()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {tn(meta().total, S.tools.search.oneMatch, S.tools.search.manyMatches)}
                {" · "}
                {tn(groups().length, S.tools.search.oneFile, S.tools.search.manyFiles)}
                <Show when={meta().truncated}>
                  {" · "}
                  {t(S.tools.search.truncated)}
                </Show>
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={groups().length > 0}>
        <Disclosure
          label={t(S.tools.card.result)}
          hint={tn(groups().length, S.tools.search.oneFile, S.tools.search.manyFiles)}
          open
        >
          <ul class="flex flex-col gap-2xs">
            <For each={groups()}>
              {(group) => (
                <li class="rounded-panel bg-surface px-sm py-2xs">
                  <FilePath path={group.path} line={group.matches[0]?.line} />
                  <div class="mt-3xs overflow-x-auto">
                    <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
                      <For each={group.matches.slice(0, PEEK)}>
                        {(match) => <MatchRow path={group.path} line={match.line} text={match.text} />}
                      </For>
                    </div>
                  </div>
                  <Show when={group.matches.length > PEEK}>
                    <p class="mt-3xs text-2xs text-faint">
                      {t(S.tools.search.more, { n: group.matches.length - PEEK })}
                    </p>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Disclosure>
      </Show>
    </ToolShell>
  );
}

/** One matching line; the whole row is clickable, because the place wanted is this line, not the file. */
function MatchRow(props: { path: string; line: number; text: string }) {
  const actions = useTranscriptActions();
  const open = () => actions.openFile;
  return (
    <Show
      when={open()}
      fallback={
        <div class="flex items-start gap-sm">
          <span class="w-10 shrink-0 text-right text-faint tabular-nums select-none">{props.line}</span>
          <span class="whitespace-pre text-text">{props.text}</span>
        </div>
      }
    >
      {(go) => (
        <button
          type="button"
          onClick={() => go()(props.path, props.line)}
          title={t(S.tools.openFileAt, { path: props.path, n: props.line })}
          class="flex w-full items-start gap-sm rounded-btn text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
        >
          <span class="w-10 shrink-0 text-right text-faint tabular-nums select-none">{props.line}</span>
          <span class="whitespace-pre text-text">{props.text}</span>
        </button>
      )}
    </Show>
  );
}

/** The `glob` card: a list of paths only, with no content lines. */
export function GlobCard(props: { call: ToolCall }) {
  const search = () => props.call.meta?.search;
  const paths = () => search()?.paths ?? [];

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">{pattern(props.call)}</code>
          <Show when={search()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {tn(meta().total, S.tools.search.oneFile, S.tools.search.manyFiles)}
                <Show when={meta().truncated}>
                  {" · "}
                  {t(S.tools.search.truncated)}
                </Show>
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={paths().length > 0}>
        <Disclosure label={t(S.tools.search.paths)} hint={`${paths().length}`}>
          <ul class="max-h-56 overflow-auto rounded-panel bg-surface px-sm py-2xs">
            <For each={paths()}>
              {(path) => (
                <li class="flex">
                  <FilePath path={path} />
                </li>
              )}
            </For>
          </ul>
        </Disclosure>
      </Show>
    </ToolShell>
  );
}
