import { createSignal, onMount } from "solid-js";
import { S, t } from "../lib/i18n";
import type { IconName } from "./Icon";
import DialogShell, { Button } from "./projects/DialogShell";

/** Ask for *one line of text*, nothing more. Built on `DialogShell` for the invisible parts (focus trap, Esc,
 * focus restore); it preselects the old value, names the action on the button, and disables it when empty. */
export default function PromptDialog(props: {
  title: string;
  desc?: string;
  /** Input label; it describes the *value*, it does not repeat the title. */
  label: string;
  /** Initial value, normally the current one rather than an empty field. */
  value: string;
  placeholder?: string;
  confirmLabel: string;
  icon?: IconName;
  onConfirm: (value: string) => void;
  onClose: () => void;
}) {
  let input: HTMLInputElement | undefined;
  const [value, setValue] = createSignal(props.value);
  const trimmed = () => value().trim();

  const submit = () => {
    if (trimmed() === "") return;
    props.onConfirm(trimmed());
  };

  onMount(() => {
    // Deferred by a microtask: the shell focuses this field *after* this code, and focusing clears a selection.
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  return (
    <DialogShell
      icon={props.icon ?? "pencil"}
      title={props.title}
      desc={props.desc}
      onClose={props.onClose}
      footer={() => (
        <>
          <Button onClick={props.onClose}>{t(S.common.cancel)}</Button>
          <Button variant="primary" onClick={submit} disabled={trimmed() === ""}>
            {props.confirmLabel}
          </Button>
        </>
      )}
    >
      <label class="flex flex-col gap-2xs">
        <span class="text-xs text-muted">{props.label}</span>
        <input
          ref={input}
          type="text"
          value={value()}
          placeholder={props.placeholder}
          spellcheck={false}
          autocapitalize="off"
          autocomplete="off"
          onInput={(event) => setValue(event.currentTarget.value)}
          onKeyDown={(event) => {
            // Enter confirms; `preventDefault` so the key carries exactly one meaning and no browser default.
            if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
          class="h-(--cta-h) min-w-0 rounded-btn border border-line-strong bg-bg px-sm text-sm text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        />
      </label>
    </DialogShell>
  );
}
