import { createSignal, createUniqueId, Show, type JSX } from "solid-js";
import { S, t } from "../../lib/i18n";
import type { Msg } from "../../lib/i18n";
import type { ToolCall } from "../../lib/protocol";
import Icon from "../Icon";
import { Disclosure, StateDot, stateLabel, type DotState } from "../primitives";

/** Labels for wire tool names; an unknown name is shown as it is. */
const TOOL_LABEL: Record<string, Msg> = {
  read: S.tools.name.read,
  write: S.tools.name.write,
  edit: S.tools.name.edit,
  glob: S.tools.name.glob,
  grep: S.tools.name.grep,
  bash: S.tools.name.bash,
  job_output: S.tools.name.jobOutput,
  job_kill: S.tools.name.jobKill,
  todo_write: S.tools.name.todoWrite,
};

// Translated at the call site, so the label follows the language without changing the signature.
export const toolLabel = (name: string): string => {
  const msg = TOOL_LABEL[name];
  return msg === undefined ? name : t(msg);
};

/** Shorten an argument value to a single skimmable line. */
function short(value: unknown, max = 48): string {
  const text =
    typeof value === "string" ? value : value === undefined ? "" : JSON.stringify(value) ?? "";
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

function summarizeArgs(args: unknown): string {
  if (args === null || typeof args !== "object") return short(args);
  return Object.entries(args as Record<string, unknown>)
    .map(([key, value]) => `${key}=${short(value)}`)
    .join(" · ");
}

export function prettyArgs(args: unknown): string {
  try {
    return JSON.stringify(args, null, 2) ?? "null";
  } catch {
    return String(args);
  }
}

const DEFAULT_STATE: Record<ToolCall["state"], DotState> = {
  running: "running",
  ok: "ok",
  error: "error",
};

/** Shared frame for every tool card: one fixed header row, and the whole row folds. */
export function ToolShell(props: {
  call: ToolCall;
  /** Override the state dot: `bash` needs red on a non-zero exit, which is not a tool error. */
  state?: DotState;
  summary?: JSX.Element;
  defaultOpen?: boolean;
  children?: JSX.Element;
}) {
  const [open, setOpen] = createSignal(props.defaultOpen ?? true);
  const bodyId = createUniqueId();

  return (
    <section
      class="ml-[calc(var(--avatar)+var(--sp-md))] flex flex-col overflow-hidden rounded-panel border border-line bg-surface-soft"
      aria-label={t(S.tools.card.aria, {
        name: toolLabel(props.call.name),
        state: t(stateLabel(DEFAULT_STATE[props.call.state])),
      })}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={props.children ? open() : undefined}
        aria-controls={props.children ? bodyId : undefined}
        disabled={!props.children}
        class="flex min-w-0 items-center gap-sm px-sm py-2xs text-left transition-colors duration-[var(--dur-fast)] enabled:hover:bg-[var(--overlay-faint)] disabled:cursor-default"
      >
        <StateDot state={props.state ?? DEFAULT_STATE[props.call.state]} />
        <span class="shrink-0 text-xs font-medium text-ink">{toolLabel(props.call.name)}</span>
        {/* Show the wire name only when it differs from the label, or MCP names print twice. */}
        <Show when={toolLabel(props.call.name) !== props.call.name}>
          <code class="shrink-0 rounded-btn bg-[var(--overlay-faint)] px-3xs font-mono text-2xs text-faint">
            {props.call.name}
          </code>
        </Show>
        <Show when={props.summary}>
          <span class="min-w-0 flex-1 truncate text-xs text-muted">{props.summary}</span>
        </Show>
        <Show when={props.children}>
          <span class="ml-auto shrink-0 text-faint">
            <Icon
              name="chevron-right"
              size={13}
              class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
            />
          </span>
        </Show>
      </button>
      <Show when={props.children}>
        <div id={bodyId} hidden={!open()} class="flex flex-col gap-xs border-t border-line px-sm py-sm">
          {props.children}
        </div>
      </Show>
    </section>
  );
}

/** Fallback card: an MCP tool has no card of its own, so always show name, args and result. */
export default function GenericToolCard(props: { call: ToolCall }) {
  return (
    <ToolShell call={props.call} summary={summarizeArgs(props.call.args)} defaultOpen={false}>
      <Disclosure label={t(S.tools.card.args)}>
        <pre class="max-h-64 overflow-auto rounded-panel bg-surface px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
          {prettyArgs(props.call.args)}
        </pre>
      </Disclosure>
      <Show when={props.call.preview}>
        {(preview) => (
          <Disclosure label={t(S.tools.card.result)} hint={t(S.tools.card.chars, { n: preview().length })}>
            <pre class="max-h-64 overflow-auto rounded-panel bg-surface px-sm py-2xs font-mono text-2xs whitespace-pre-wrap text-text">
              {preview()}
            </pre>
          </Disclosure>
        )}
      </Show>
    </ToolShell>
  );
}
