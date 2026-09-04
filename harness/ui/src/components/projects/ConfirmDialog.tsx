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
          <Button variant="danger" onClick={props.onConfirm} disabled={props.busy}>
            {props.confirmLabel}
          </Button>
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
