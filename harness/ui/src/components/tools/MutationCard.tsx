import { Show } from "solid-js";
import { S, t } from "../../lib/i18n";
import type { DiffHunk, ToolCall } from "../../lib/protocol";
import DiffBlock from "../DiffBlock";
import { FilePath } from "../primitives";
import { ToolShell } from "./ToolCard";

/** Card for `edit` and `write`: intended diff while running, the applied diff once done, none on error. */
export default function MutationCard(props: { call: ToolCall }) {
  const diffs = (): DiffHunk[] | null => {
    if (props.call.state === "error") return null;
    const applied = props.call.meta?.diffs;
    if (applied && applied.length > 0) return applied;
    const intended = props.call.intendedDiffs;
    return intended && intended.length > 0 ? intended : null;
  };

  const path = () => {
    const bag = props.call.args as Record<string, unknown> | null;
    const fromArgs = bag && typeof bag.file_path === "string" ? bag.file_path : null;
    return fromArgs ?? diffs()?.[0]?.path ?? "";
  };

  return (
    <ToolShell
      call={props.call}
      summary={
        <span class="flex min-w-0 items-center gap-sm">
          <FilePath path={path()} line={diffs()?.[0]?.new_start ?? undefined} />
          <Show when={props.call.state === "running" && diffs()}>
            <span class="shrink-0 text-2xs text-warn">{t(S.tools.mutation.intended)}</span>
          </Show>
        </span>
      }
    >
      <Show when={diffs()}>{(list) => <DiffBlock diffs={list()} />}</Show>
      <Show when={props.call.state === "error" && props.call.preview}>
        {(message) => (
          <p class="rounded-panel bg-danger-soft px-sm py-2xs text-xs text-danger">{message()}</p>
        )}
      </Show>
    </ToolShell>
  );
}
