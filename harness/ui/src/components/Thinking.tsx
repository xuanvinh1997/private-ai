import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { S, t } from "../lib/i18n";
import type { ConversationNode } from "../lib/protocol";
import Icon from "./Icon";
import { toolLabel } from "./tools/ToolCard";

/** The "assistant is working" indicator, filling the silence a local model's load time creates. It switches off
 * once text streams, and its label follows the current step so it answers "waiting on what". */
export default function Thinking(props: { nodes: ConversationNode[]; busy: boolean }) {
  const last = () => props.nodes[props.nodes.length - 1];

  const show = () => {
    if (!props.busy) return false;
    const node = last();
    return !(node?.kind === "assistant" && node.streaming);
  };

  /** Most recent running tool, found by scanning backwards, since todos and notices often follow a `tool_start`. */
  const running = () => {
    for (let i = props.nodes.length - 1; i >= 0; i--) {
      const node = props.nodes[i]!;
      if (node.kind === "tool" && node.call.state === "running") return node.call.name;
    }
    return null;
  };

  const label = () => {
    const name = running();
    if (name !== null) {
      const pretty = toolLabel(name);
      // Known tools get a phrase; unknown names (usually MCP) keep their original form, which lower-casing would harm.
      return pretty === name
        ? t(S.chat.thinking.running, { name })
        : t(S.chat.thinking.doing, { what: pretty.toLowerCase() });
    }
    const node = last();
    if (node?.kind === "progress") return node.label;
    return t(S.chat.thinking.idle);
  };

  // The clock counts from the start of the turn, not the phase: a counter resetting per tool answers the wrong question.
  const [secs, setSecs] = createSignal(0);
  createEffect(() => {
    if (!props.busy) {
      setSecs(0);
      return;
    }
    const start = Date.now();
    setSecs(0);
    const timer = setInterval(() => setSecs(Math.floor((Date.now() - start) / 1000)), 1000);
    onCleanup(() => clearInterval(timer));
  });

  return (
    <Show when={show()}>
      {/* `role="status"` plus `aria-live="polite"`: announced once per label change, never interrupting. */}
      <div class="flex gap-md" role="status" aria-live="polite">
        <div
          aria-hidden="true"
          class="mt-3xs grid size-(--avatar) shrink-0 place-items-center rounded-pill bg-surface-hover text-accent-ink"
        >
          <Icon name="sparkle" size={15} />
        </div>

        <div class="flex min-w-0 items-center gap-sm">
          <span class="min-w-0 truncate text-sm text-muted">{label()}</span>
          <span class="pai-dots flex shrink-0 items-center gap-3xs" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          {/* The number appears only once the wait is long enough to be a question. */}
          <Show when={secs() >= 3}>
            <span class="shrink-0 text-2xs text-faint tabular-nums">{secs()}s</span>
          </Show>
        </div>
      </div>
    </Show>
  );
}
