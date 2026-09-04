import { createUniqueId, Show, type JSX } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon, { type IconName } from "../Icon";
import { InfoDot } from "../settings/FormKit";

/** Shared dialog shell for the project and library screens; it exists for the invisible parts, focus trap, Esc to close and focus restore, and `footer` is a function because a JSX prop read twice builds two sets of buttons. */
export default function DialogShell(props: {
  icon: IconName;
  title: string;
  desc?: string;
  /** The long text behind the title, kept in an `InfoDot` rather than spread over the dialog. */
  more?: string;
  tone?: "accent" | "danger";
  /** Work is running in the dialog; screen readers need to know before they read it out. */
  busy?: boolean;
  width?: "md" | "lg";
  onClose: () => void;
  children: JSX.Element;
  footer: () => JSX.Element;
}) {
  let panel: HTMLDivElement | undefined;
  const titleId = createUniqueId();
  const descId = createUniqueId();

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-[var(--z-modal)] flex justify-center overflow-y-auto p-4xl"
      style={{ background: "var(--scrim)" }}
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={props.desc === undefined ? undefined : descId}
        aria-busy={props.busy === true ? "true" : "false"}
        class="my-auto flex w-full flex-col gap-md rounded-card border border-line bg-surface p-(--dialog-pad-x) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        classList={{
          "max-w-[560px]": (props.width ?? "md") === "md",
          "max-w-[680px]": props.width === "lg",
        }}
      >
        <div class="flex items-start gap-sm">
          <span
            class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel"
            classList={{
              "bg-accent-soft text-accent-ink": (props.tone ?? "accent") === "accent",
              "bg-danger-soft text-danger": props.tone === "danger",
            }}
          >
            <Icon name={props.icon} size={16} />
          </span>
          <div class="flex min-w-0 flex-col gap-3xs">
            <h2 id={titleId} class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
              {props.title}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </h2>
            <Show when={props.desc}>
              {(text) => (
                <p id={descId} class="m-0 text-xs text-muted">
                  {text()}
                </p>
              )}
            </Show>
          </div>
        </div>

        {props.children}

        <div class="flex flex-wrap items-center justify-end gap-sm">{props.footer()}</div>
      </div>
    </div>
  );
}

/** The button variants reused across dialogs and the project screen, so equal roles stay equal. */
export function Button(props: {
  children: JSX.Element;
  onClick?: () => void;
  variant?: "primary" | "ghost" | "outline" | "danger";
  disabled?: boolean;
  icon?: IconName;
  label?: string;
  title?: string;
  type?: "button" | "submit";
}) {
  const variant = () => props.variant ?? "ghost";
  return (
    <button
      type={props.type ?? "button"}
      onClick={() => props.onClick?.()}
      disabled={props.disabled}
      aria-label={props.label}
      title={props.title}
      class="pai-btn shrink-0 text-xs"
      classList={{
        "pai-btn-primary": variant() === "primary",
        "pai-btn-ghost": variant() === "ghost",
        "pai-btn-secondary enabled:hover:border-accent enabled:hover:bg-accent-soft enabled:hover:text-accent-ink":
          variant() === "outline",
        "pai-btn-danger": variant() === "danger",
      }}
    >
      <Show when={props.icon}>{(name) => <Icon name={name()} size={14} />}</Show>
      {props.children}
    </button>
  );
}
