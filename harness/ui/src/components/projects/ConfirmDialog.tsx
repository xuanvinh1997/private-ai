import { Show } from "solid-js";
import { S, t } from "../../lib/i18n";
import type { IconName } from "../Icon";
import DialogShell, { Button } from "./DialogShell";

/** Confirmation for irreversible actions: the caller names the confirm button, because people read the button and not the question, and focus lands on Cancel so a stray Enter cannot destroy anything. */
export default function ConfirmDialog(props: {
  title: string;
  /** The sentence saying what happens and what does not. */
  body: string;
  /** The rest of the reassurance, behind the question mark next to the title. */
  more?: string;
  detail?: string;
  confirmLabel: string;
  /** A heavier second action, when the question really has two answers rather than one.
   *
   * Given one, the dialog demotes `confirm` to an outline button and paints this one as the destructive
   * choice: the reader picks by weight, and the heavier-looking button must be the one that keeps less. */
  escalate?: { label: string; onClick: () => void };
  icon?: IconName;
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <DialogShell
      icon={props.icon ?? "warn"}
      tone="danger"
      title={props.title}
      more={props.more}
      busy={props.busy}
      onClose={props.onClose}
      footer={() => (
        <>
          <Button onClick={props.onClose} disabled={props.busy}>
            {t(S.common.cancel)}
          </Button>
          <Button
            variant={props.escalate ? "outline" : "danger"}
            onClick={props.onConfirm}
            disabled={props.busy}
          >
            {props.confirmLabel}
          </Button>
          <Show when={props.escalate}>
            {(heavier) => (
              <Button variant="danger" onClick={heavier().onClick} disabled={props.busy}>
                {heavier().label}
              </Button>
            )}
          </Show>
        </>
      )}
    >
      <p class="m-0 text-sm text-text">{props.body}</p>
      <Show when={props.detail}>
        {(text) => (
          <p
            class="m-0 min-w-0 truncate rounded-panel bg-surface-soft px-sm py-2xs font-mono text-2xs text-muted"
            dir="rtl"
            title={text()}
          >
            <bdi>{text()}</bdi>
          </p>
        )}
      </Show>
    </DialogShell>
  );
}
