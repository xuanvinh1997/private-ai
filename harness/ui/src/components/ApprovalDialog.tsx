import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { useFocusTrap } from "../hooks/useFocusTrap";
import type { PendingApproval } from "../lib/conversation";
import { intendedDiffs } from "../lib/diff";
import { S, t } from "../lib/i18n";
import type { ApprovalDecision } from "../lib/protocol";
import DiffBlock from "./DiffBlock";
import Icon from "./Icon";
import { prettyArgs, toolLabel } from "./tools/ToolCard";

/** Default timeout when the core names none; a dialog that waits forever blocks the whole turn. */
const DEFAULT_TIMEOUT_MS = 120_000;

/** Tool-call approval dialog, strictly fail-closed: close, Esc, timeout and unmount all mean *reject*, and only a
 * deliberate click allows. There is no "remember my choice": one allow is one allow. */
export default function ApprovalDialog(props: {
  request: PendingApproval;
  onDecide: (decision: ApprovalDecision) => void;
}) {
  let panel: HTMLDivElement | undefined;
  let rejectButton: HTMLButtonElement | undefined;
  let answered = false;

  const budget = () => props.request.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const [left, setLeft] = createSignal(budget());

  const decide = (decision: ApprovalDecision) => {
    if (answered) return;
    answered = true;
    props.onDecide(decision);
  };

  useFocusTrap(
    () => panel,
    () => decide("rejected"),
  );

  onMount(() => {
    // The focus trap lands on the diff's copy button; pull focus to Reject, so the safe option is the default.
    rejectButton?.focus();

    const started = Date.now();
    const tick = setInterval(() => {
      const remaining = budget() - (Date.now() - started);
      setLeft(remaining);
      if (remaining <= 0) decide("rejected");
    }, 250);
    onCleanup(() => clearInterval(tick));
  });

  // Unmounting without an answer means reject; this is the last net, covering session switches and hot reload.
  onCleanup(() => decide("rejected"));

  const seconds = () => Math.max(0, Math.ceil(left() / 1000));
  const diffs = () => intendedDiffs(props.request.name, props.request.args);

  return (
    <div
      class="fixed inset-0 z-[var(--z-modal)] flex items-center justify-center p-lg"
      style={{ background: "var(--scrim)" }}
      // A click outside is also a rejection, not a way to skip the question.
      onClick={(event) => {
        if (event.target === event.currentTarget) decide("rejected");
      }}
    >
      <div
        ref={panel}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="approval-title"
        aria-describedby="approval-body"
        class="flex max-h-full w-full max-w-[560px] flex-col gap-(--dialog-gap) overflow-auto rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop"
      >
        {/* The shield leads the question: the app's standard dialog header, and it reads as a permission prompt. */}
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name="shield" size={16} />
          </span>
          <h2 id="approval-title" class="m-0 flex-1 text-lg font-medium text-ink">
            {t(S.chat.approval.title, { tool: toolLabel(props.request.name) })}
          </h2>
        </div>

        <div id="approval-body" class="flex flex-col gap-sm">
          {/* The tool name sits *inside* the sentence; split around an element, the halves cannot reorder per language. */}
          <p class="m-0 text-sm text-muted">
            {t(S.chat.approval.body, { name: props.request.name })}
            <Show when={props.request.reason}>
              {(reason) => <span class="block text-text">{reason()}</span>}
            </Show>
          </p>

          <Show when={diffs()}>
            {(list) => <DiffBlock diffs={list()} maxLines={12} />}
          </Show>

          <pre class="max-h-56 overflow-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
            {prettyArgs(props.request.args)}
          </pre>

          <p class="m-0 text-2xs text-faint" role="timer" aria-live="off">
            {t(S.chat.approval.timeout, { n: seconds() })}
          </p>
        </div>

        <div class="flex justify-end gap-sm">
          {/* Reject comes first and takes focus: the safe option must also be the easiest one. */}
          <button
            ref={rejectButton}
            type="button"
            onClick={() => decide("rejected")}
            class="pai-btn pai-btn-cta pai-btn-secondary"
          >
            {t(S.chat.approval.reject)}
          </button>
          <button
            type="button"
            onClick={() => decide("allowed_once")}
            class="pai-btn pai-btn-cta pai-btn-primary"
          >
            {t(S.chat.approval.allowOnce)}
          </button>
        </div>
      </div>
    </div>
  );
}
