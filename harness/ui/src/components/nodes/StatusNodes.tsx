import { Show } from "solid-js";
import type { NodeProps } from "../../lib/registry";
import Icon from "../Icon";

/** A notice outside the reply, indented to the message text axis rather than a second margin. */
export function NoticeNode(props: NodeProps<"notice">) {
  return (
    <p
      class="m-0 flex items-center gap-sm pl-[calc(var(--avatar)+var(--sp-md))] text-xs text-faint"
      role="note"
    >
      <span class="h-px flex-none w-lg bg-line" aria-hidden="true" />
      {props.node.message}
    </p>
  );
}

export function ProgressNode(props: NodeProps<"progress">) {
  return (
    <p
      class="m-0 flex items-center gap-sm pl-[calc(var(--avatar)+var(--sp-md))] text-xs text-muted"
      role="status"
    >
      <span class="size-1.5 shrink-0 rounded-pill bg-accent motion-safe:animate-pulse" aria-hidden="true" />
      {props.node.label}
      <Show when={props.node.detail}>
        {(detail) => <span class="min-w-0 truncate font-mono text-2xs text-faint">{detail()}</span>}
      </Show>
    </p>
  );
}

/** Turn-level error: `role="alert"`, not polite, because a broken turn must be heard at once. */
export function ErrorNode(props: NodeProps<"error">) {
  return (
    <div
      role="alert"
      class="ml-[calc(var(--avatar)+var(--sp-md))] flex items-start gap-sm rounded-panel border border-danger-soft bg-danger-soft px-md py-xs text-sm text-danger"
    >
      <span class="mt-3xs shrink-0">
        <Icon name="x" size={14} />
      </span>
      <span class="min-w-0">{props.node.message}</span>
    </div>
  );
}
