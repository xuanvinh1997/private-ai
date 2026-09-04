import { Show } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import { S, t } from "../../lib/i18n";
import Icon from "./../Icon";
import { InfoDot } from "../settings/FormKit";

/** Confirmation dialog for an irreversible action, shared with the MCP screen so the keyboard rules exist once; Cancel takes focus first and Esc closes, so a reflex Enter costs nothing. */
export default function ConfirmDialog(props: {
  title: string;
  body: string;
  /** The long explanation behind the question, kept in an `InfoDot` next to the title. */
  more?: string;
  /** A secondary line of machine detail (path, command, name), shown in a mono font. */
  detail?: string;
  confirmLabel: string;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  // No focus steering needed: the first button in the panel is Cancel, which is the safe default.
  let panel: HTMLDivElement | undefined;

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-[var(--z-modal)] flex items-center justify-center p-lg"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-body"
        class="flex w-full max-w-[440px] flex-col gap-(--dialog-gap) rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
      >
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-danger-soft text-danger">
            <Icon name="warn" size={16} />
          </span>
          <div class="flex min-w-0 flex-col gap-3xs">
            <h2 id="confirm-title" class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
              {props.title}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </h2>
            <p id="confirm-body" class="m-0 text-xs text-muted">
              {props.body}
            </p>
          </div>
        </div>

        <Show when={props.detail}>
          {(detail) => (
            <p class="m-0 overflow-x-auto rounded-panel border border-line bg-surface-soft px-sm py-2xs font-mono text-2xs whitespace-pre text-text">
              {detail()}
            </p>
          )}
        </Show>

        <div class="flex justify-end gap-sm">
          <button
            type="button"
            onClick={props.onClose}
            class="pai-btn pai-btn-secondary text-xs"
          >
            {t(S.common.cancel)}
          </button>
          <button
            type="button"
            disabled={props.busy}
            aria-busy={props.busy}
            onClick={props.onConfirm}
            class="pai-btn pai-btn-danger text-xs"
          >
            {props.confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
