import { For, Show } from "solid-js";
import { S, t, tn } from "../../lib/i18n";
import type { ToolCall } from "../../lib/protocol";
import { Disclosure, FilePath } from "../primitives";
import { ToolShell } from "./ToolCard";

/** The `read` card: path plus lines read over total, which says whether the model saw the whole file. */
export default function ReadCard(props: { call: ToolCall }) {
  const read = () => props.call.meta?.read;
  const path = () =>
    read()?.path ??
    (typeof (props.call.args as Record<string, unknown> | null)?.file_path === "string"
      ? String((props.call.args as Record<string, unknown>).file_path)
      : "");

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          {/* Open at the window the model saw: that is where its answer came from. */}
          <FilePath path={path()} line={read()?.offset || undefined} />
          <Show when={read()}>
            {(meta) => (
              <span class="shrink-0 tabular-nums text-faint">
                {t(S.tools.read.lines, {
                  n: meta().lines.length,
                  total: meta().total_lines,
                })}
                <Show when={meta().offset > 0}>
                  {" · "}
                  {t(S.tools.read.fromLine, { n: meta().offset })}
                </Show>
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={read()}>
        {(meta) => (
          <Disclosure
            label={t(S.tools.read.content)}
            hint={tn(meta().lines.length, S.tools.read.oneLine, S.tools.read.manyLines)}
          >
            <div class="max-h-72 overflow-auto rounded-panel bg-surface">
              <div class="w-max min-w-full font-mono text-2xs leading-[1.55]">
                <For each={meta().lines}>
                  {(line) => (
                    <div class="flex items-start gap-sm px-sm">
                      <span
                        aria-hidden="true"
                        class="w-10 shrink-0 text-right text-faint tabular-nums select-none"
                      >
                        {line.number}
                      </span>
                      <span class="whitespace-pre text-text">{line.text === "" ? " " : line.text}</span>
                    </div>
                  )}
                </For>
              </div>
            </div>
          </Disclosure>
        )}
      </Show>
    </ToolShell>
  );
}
