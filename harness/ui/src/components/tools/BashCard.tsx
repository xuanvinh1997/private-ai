import { createMemo, createSignal, Show } from "solid-js";
import { S, t } from "../../lib/i18n";
import type { ToolCall } from "../../lib/protocol";
import { CopyButton, IconButton, type DotState } from "../primitives";
import { ToolShell } from "./ToolCard";

/** Same fold arithmetic as the diff block: the head half rounds up. */
const MAX_LINES = 12;

function fold(text: string, expanded: boolean): { body: string; hidden: number } {
  const lines = text.split("\n");
  if (expanded || lines.length <= MAX_LINES) return { body: text, hidden: 0 };
  const head = Math.ceil(MAX_LINES / 2);
  const tail = MAX_LINES - head;
  const hidden = lines.length - head - tail;
  return {
    body: [
      ...lines.slice(0, head),
      t(S.tools.bash.hidden, { n: hidden }),
      ...lines.slice(lines.length - tail),
    ].join("\n"),
    hidden,
  };
}

/** The `bash` card: a non-zero exit shows red, and a background job with no exit code shows running. */
export default function BashCard(props: { call: ToolCall }) {
  const [expanded, setExpanded] = createSignal(false);
  const terminal = () => props.call.meta?.terminal;
  const command = () => {
    const bag = props.call.args as Record<string, unknown> | null;
    const fromArgs = bag && typeof bag.command === "string" ? bag.command : null;
    return terminal()?.command ?? fromArgs ?? "";
  };
  const output = () => terminal()?.output ?? props.call.preview ?? "";
  const folded = createMemo(() => fold(output(), expanded()));

  const state = (): DotState => {
    if (props.call.state === "running") return "running";
    if (props.call.state === "error") return "error";
    const code = terminal()?.exit_code;
    if (code === null || code === undefined) return terminal()?.background ? "running" : "ok";
    return code === 0 ? "ok" : "error";
  };

  return (
    <ToolShell
      call={props.call}
      state={state()}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <code class="min-w-0 truncate font-mono text-xs text-accent-ink">$ {command()}</code>
          <Show when={terminal()?.background}>
            <span class="shrink-0 rounded-pill bg-warn-soft px-2xs text-2xs text-warn">
              {t(S.tools.bash.background)}
            </span>
          </Show>
          <Show when={terminal()?.exit_code !== null && terminal()?.exit_code !== undefined}>
            <span
              class="shrink-0 tabular-nums text-2xs"
              classList={{
                "text-success": terminal()?.exit_code === 0,
                "text-danger": terminal()?.exit_code !== 0,
              }}
            >
              exit {terminal()?.exit_code}
            </span>
          </Show>
          <Show when={terminal()?.signal}>
            {(signal) => (
              <span class="shrink-0 text-2xs text-danger">
                {t(S.tools.bash.signal, { name: signal() })}
              </span>
            )}
          </Show>
        </span>
      }
    >
      <Show when={terminal()?.cwd}>
        {(cwd) => (
          <p class="font-mono text-2xs text-faint">{t(S.tools.bash.cwd, { path: cwd() })}</p>
        )}
      </Show>
      <Show when={output() !== ""}>
        <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
          <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
            <figcaption class="text-2xs text-muted">{t(S.tools.bash.output)}</figcaption>
            <div class="flex items-center gap-3xs">
              {/* An icon, not a word: this button repeats on every command card. */}
              <Show when={folded().hidden > 0 || expanded()}>
                <IconButton
                  icon={expanded() ? "fold" : "unfold"}
                  label={expanded() ? t(S.tools.bash.collapse) : t(S.tools.bash.expand)}
                  size="sm"
                  expanded={expanded()}
                  onClick={() => setExpanded((v) => !v)}
                />
              </Show>
              <CopyButton text={output} label={t(S.tools.bash.copyOutput)} />
            </div>
          </div>
          <div class="max-h-72 overflow-auto">
            <pre class="w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] whitespace-pre text-text">
              {folded().body}
            </pre>
          </div>
        </figure>
      </Show>
      <Show when={props.call.state === "running" && terminal()?.background}>
        <p class="text-2xs text-faint">
          {terminal()?.job_id
            ? t(S.tools.bash.job, { id: String(terminal()?.job_id) })
            : t(S.tools.bash.jobless)}
        </p>
      </Show>
    </ToolShell>
  );
}
