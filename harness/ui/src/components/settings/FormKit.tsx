import { createSignal, createUniqueId, For, Show, type JSX } from "solid-js";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import Icon, { type IconName } from "./../Icon";
import { S, t } from "../../lib/i18n";

/** The shared vocabulary of every settings page: rows, groups, toggles, selects and dialogs, kept in one place so the invisible details stay identical. Every JSX-bearing prop here is a function, because a JSX prop read twice builds two overlapping copies; `children` is the compiler's exception. */

/** Dialog frame: scrim, focus trap, Esc to close, Enter to submit. */
export function DialogShell(props: {
  title: string;
  desc?: string;
  /** The long text after `desc`, kept in an `InfoDot` beside the dialog title. */
  more?: string;
  icon: IconName;
  /** The MCP form has two columns of inputs and does not fit the default width. */
  wide?: boolean;
  labelledBy?: string;
  onSubmit?: () => void;
  onClose: () => void;
  footer: () => JSX.Element;
  children: JSX.Element;
}) {
  let panel: HTMLDivElement | undefined;
  const titleId = createUniqueId();

  useFocusTrap(() => panel, props.onClose);

  return (
    <div
      class="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto p-2xl"
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
        class="my-auto flex w-full flex-col gap-(--dialog-gap) rounded-card border border-line bg-surface px-(--dialog-pad-x) py-(--dialog-pad-y) shadow-pop motion-safe:animate-[pai-pop_var(--dur-fast)_var(--ease-out)]"
        classList={{ "max-w-[560px]": !props.wide, "max-w-[720px]": props.wide === true }}
      >
        <div class="flex items-start gap-sm">
          <span class="mt-3xs grid size-8 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
            <Icon name={props.icon} size={16} />
          </span>
          <div class="flex min-w-0 flex-1 flex-col gap-3xs">
            <h2 id={titleId} class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
              {props.title}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </h2>
            <Show when={props.desc}>
              {(desc) => <p class="m-0 text-xs text-muted">{desc()}</p>}
            </Show>
          </div>
        </div>

        {/* A real `<form>`, so Enter in any field submits, which is the keyboard reflex. */}
        <form
          class="flex flex-col gap-(--dialog-gap)"
          onSubmit={(event) => {
            event.preventDefault();
            props.onSubmit?.();
          }}
        >
          {props.children}
          <div class="flex flex-wrap items-center justify-end gap-sm">{props.footer()}</div>
        </form>
      </div>
    </div>
  );
}

const INPUT_CLASS =
  "h-(--control-h) w-full rounded-btn border border-line-strong bg-bg px-sm text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent disabled:cursor-not-allowed disabled:opacity-50";

/** A labelled input; the label is a real `<label>`, not a `<span>` placed above it. */
export function TextField(props: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  hint?: string;
  /** The long sentence after `hint`, in an `InfoDot` beside the label rather than under the field. */
  more?: string;
  placeholder?: string;
  type?: "text" | "password";
  mono?: boolean;
  disabled?: boolean;
  invalid?: boolean;
  autocomplete?: string;
  /** Hide the label visually, not from screen readers, for fields inside an already-labelled `<Row>`. */
  hideLabel?: boolean;
  ref?: (el: HTMLInputElement) => void;
}) {
  const id = createUniqueId();
  const hintId = createUniqueId();
  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      <Show
        when={props.more !== undefined && props.hideLabel !== true}
        fallback={
          <label for={id} class={props.hideLabel === true ? "sr-only" : "text-2xs text-faint"}>
            {props.label}
          </label>
        }
      >
        <span class="flex items-center gap-2xs text-2xs text-faint">
          <label for={id}>{props.label}</label>
          <InfoDot text={props.more ?? ""} label={t(S.settings.form.about, { label: props.label })} />
        </span>
      </Show>
      <input
        id={id}
        ref={props.ref}
        type={props.type ?? "text"}
        value={props.value}
        placeholder={props.placeholder}
        disabled={props.disabled}
        spellcheck={false}
        autocapitalize="off"
        autocomplete={props.autocomplete ?? "off"}
        aria-invalid={props.invalid}
        aria-describedby={props.hint === undefined ? undefined : hintId}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        class={`${INPUT_CLASS} ${props.mono ? "font-mono" : ""}`}
        classList={{ "border-danger": props.invalid === true }}
      />
      <Show when={props.hint}>
        {(hint) => (
          <p id={hintId} class="m-0 text-2xs text-faint">
            {hint()}
          </p>
        )}
      </Show>
    </div>
  );
}

/** A multi-line field, used only for pasting JSON, so it has no variants. */
export function TextArea(props: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  placeholder?: string;
  rows?: number;
  invalid?: boolean;
}) {
  const id = createUniqueId();
  return (
    <div class="flex min-w-0 flex-col gap-2xs">
      <label for={id} class="text-2xs text-faint">
        {props.label}
      </label>
      <textarea
        id={id}
        rows={props.rows ?? 5}
        value={props.value}
        placeholder={props.placeholder}
        spellcheck={false}
        aria-invalid={props.invalid}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        class="w-full resize-y rounded-btn border border-line-strong bg-bg px-sm py-xs font-mono text-2xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
        classList={{ "border-danger": props.invalid === true }}
      />
    </div>
  );
}

/** An on/off switch with `role="switch"`, so screen readers say on or off rather than checked. */
export function Toggle(props: {
  label: string;
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  busy?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      aria-busy={props.busy}
      disabled={props.disabled || props.busy}
      onClick={() => props.onChange(!props.checked)}
      class="inline-flex h-(--control-h) w-10 shrink-0 items-center justify-center rounded-pill disabled:cursor-not-allowed disabled:opacity-50"
    >
      <span
        aria-hidden="true"
        class="relative inline-flex h-5 w-9 items-center rounded-pill border transition-colors duration-[var(--dur-fast)]"
        classList={{
          "border-accent bg-accent": props.checked,
          "border-line-strong bg-surface-soft": !props.checked,
        }}
      >
        <span
          class="size-3.5 rounded-pill transition-transform duration-[var(--dur-fast)] motion-reduce:transition-none"
          classList={{
            "translate-x-[18px] bg-on-accent": props.checked,
            "translate-x-[3px] bg-line-strong": !props.checked,
          }}
        />
      </span>
    </button>
  );
}

/** A row of mutually exclusive pills, shaped like the settings pages' `radiogroup`. */
export function PillChoice<T extends string>(props: {
  label: string;
  value: T;
  options: { id: T; label: string; icon?: IconName }[];
  onPick: (value: T) => void;
  hint?: string;
  /** The long sentence after `hint`, in an `InfoDot` beside the group's label. */
  more?: string;
}) {
  return (
    <div class="flex flex-col gap-2xs">
      <span class="flex items-center gap-2xs text-2xs text-faint">
        {props.label}
        <Show when={props.more}>
          {(more) => <InfoDot text={more()} label={t(S.settings.form.about, { label: props.label })} />}
        </Show>
      </span>
      <div
        role="radiogroup"
        aria-label={props.label}
        class="flex flex-wrap gap-2xs"
        onKeyDown={(event) => {
          const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"];
          if (!keys.includes(event.key)) return;
          event.preventDefault();
          const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]')];
          if (buttons.length === 0) return;
          const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
          const next =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? buttons.length - 1
                : (current + (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) +
                    buttons.length) %
                  buttons.length;
          buttons[next]?.focus();
          const option = props.options[next];
          if (option !== undefined) props.onPick(option.id);
        }}
      >
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              role="radio"
              aria-checked={props.value === option.id}
              tabIndex={props.value === option.id ? 0 : -1}
              onClick={() => props.onPick(option.id)}
              class="flex h-(--control-h) items-center gap-2xs rounded-pill border px-md text-xs font-medium transition-colors duration-[var(--dur-fast)]"
              classList={{
                "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                  props.value !== option.id,
                "border-accent bg-accent-soft text-accent-ink": props.value === option.id,
              }}
            >
              <Show when={option.icon}>{(icon) => <Icon name={icon()} size={13} />}</Show>
              {option.label}
            </button>
          )}
        </For>
      </div>
      <Show when={props.hint}>
        {(hint) => <p class="m-0 text-2xs text-faint">{hint()}</p>}
      </Show>
    </div>
  );
}

/** A compact settings list: rows separated by a rule rather than cards, and no scroll container of its own, since nested scrolling means two scrollbars. */
export function RowGroup(props: { children: JSX.Element }) {
  return (
    <div class="flex flex-col divide-y divide-line rounded-card border border-line bg-surface shadow-[var(--edge-top)]">
      {props.children}
    </div>
  );
}

/** One settings row; `control` and `below` are functions because a JSX prop read twice duplicates the DOM. */
export function Row(props: {
  label: string;
  desc?: string;
  /** An MCP server name is an identifier: it appears verbatim in the tool name prefix. */
  labelMono?: boolean;
  /** The row's leading icon, carrying meaning a one-line `desc` cannot hold. */
  icon?: IconName;
  /** The long explanation, kept in an `InfoDot` beside the label. */
  more?: string;
  /** A state dot or icon in front of the label. */
  lead?: () => JSX.Element;
  /** The right-column control: a toggle, a button, a select. */
  control?: () => JSX.Element;
  /** What expands under the row: a warning, a list, this row's own detail. */
  below?: () => JSX.Element;
  /** Dim the row when its item is off, while keeping it readable enough to turn back on. */
  dim?: boolean;
}) {
  return (
    <div
      class="flex flex-col gap-2xs px-(--card-pad-x) py-sm transition-colors duration-[var(--dur-fast)]"
      classList={{ "opacity-70": props.dim === true }}
    >
      <div class="flex flex-wrap items-center gap-md">
        <Show when={props.lead}>{(render) => <>{render()()}</>}</Show>
        <Show when={props.icon}>
          {(icon) => (
            <span class="grid size-7 shrink-0 place-items-center rounded-panel bg-surface-soft text-muted">
              <Icon name={icon()} size={14} />
            </span>
          )}
        </Show>
        <div class="flex min-w-0 flex-1 flex-col gap-3xs">
          <span
            class="flex min-w-0 items-center gap-2xs text-xs font-medium text-ink"
            classList={{ "font-mono": props.labelMono === true }}
          >
            {props.label}
            <Show when={props.more}>{(more) => <InfoDot text={more()} label={t(S.settings.form.about, { label: props.label })} />}</Show>
          </span>
          <Show when={props.desc}>
            {(desc) => <p class="m-0 text-2xs text-muted">{desc()}</p>}
          </Show>
        </div>
        <Show when={props.control}>
          {(render) => <div class="flex shrink-0 items-center gap-2xs">{render()()}</div>}
        </Show>
      </div>
      <Show when={props.below}>{(render) => <>{render()()}</>}</Show>
    </div>
  );
}

/** A one-of-many picker for a row's right column, using the browser's `<select>` rather than a hand-drawn menu. */
export function Select(props: {
  label: string;
  value: string;
  options: { id: string; label: string }[];
  onPick: (value: string) => void;
  disabled?: boolean;
  mono?: boolean;
  /** Take the full width instead of stopping at 280px, for dialogs where it is the only field. */
  full?: boolean;
}) {
  const WIDTH = props.full === true ? "w-full" : "max-w-[280px]";
  return (
    <select
      aria-label={props.label}
      value={props.value}
      disabled={props.disabled}
      onChange={(event) => props.onPick(event.currentTarget.value)}
      class={`h-(--control-h) ${WIDTH} min-w-0 truncate rounded-btn border border-line-strong bg-bg px-sm text-xs text-text transition-colors duration-[var(--dur-fast)] focus:border-accent disabled:cursor-not-allowed disabled:opacity-50`}
      classList={{ "font-mono": props.mono === true }}
    >
      {/* `selected` per `<option>`, since a `<select>` given a `value` before its options falls back to the first. */}
      <For each={props.options}>
        {(option) => (
          <option value={option.id} selected={option.id === props.value}>
            {option.label}
          </option>
        )}
      </For>
    </select>
  );
}

export type BannerTone = "info" | "warn" | "danger" | "accent";

/** A sentence to read before clicking; not a toast, it stays as long as the condition does. */
export function Banner(props: {
  tone: BannerTone;
  icon: IconName;
  title?: string;
  /** The long text behind the warning, kept in an `InfoDot` beside the title. */
  more?: string;
  children: JSX.Element;
  role?: "status" | "alert";
}) {
  return (
    <div
      role={props.role}
      class="flex items-start gap-sm rounded-panel border px-sm py-2xs text-2xs"
      classList={{
        "border-line bg-surface-soft text-muted": props.tone === "info",
        "border-warn bg-warn-soft text-warn": props.tone === "warn",
        "border-danger bg-danger-soft text-danger": props.tone === "danger",
        "border-accent bg-accent-soft text-accent-ink": props.tone === "accent",
      }}
    >
      <span class="mt-3xs shrink-0">
        <Icon name={props.icon} size={13} />
      </span>
      <div class="flex min-w-0 flex-col gap-3xs">
        <Show when={props.title}>
          {(title) => (
            <span class="flex items-center gap-2xs font-medium">
              {title()}
              <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
            </span>
          )}
        </Show>
        <div class="flex min-w-0 items-start gap-2xs">
          <span class="min-w-0">{props.children}</span>
          {/* With no title the dot rides the body, or a titleless banner would swallow `more`. */}
          <Show when={props.more !== undefined && props.title === undefined}>
            <InfoDot text={props.more ?? ""} />
          </Show>
        </div>
      </div>
    </div>
  );
}

/** An external link; `plugin-opener` is not installed, so `target="_blank"` is the only route, with `rel` blocking `window.opener`. */
export function ExternalLink(props: { href: string; children: JSX.Element }) {
  return (
    <a
      href={props.href}
      target="_blank"
      rel="noreferrer noopener"
      onClick={(event) => event.stopPropagation()}
      class="inline-flex items-center gap-3xs rounded-btn text-2xs text-accent-ink underline decoration-transparent underline-offset-2 transition-colors duration-[var(--dur-fast)] hover:decoration-current"
    >
      {props.children}
      <Icon name="external" size={11} />
    </a>
  );
}


/** The dot beside a label holding the long explanation, opened by hover or focus and tied to the label by `aria-describedby`, since settings pages are scanned rather than read. */
export function InfoDot(props: { text: string; label?: string }) {
  const [open, setOpen] = createSignal(false);
  const id = createUniqueId();
  return (
    <span class="relative inline-flex shrink-0 items-center">
      <button
        type="button"
        aria-label={props.label ?? t(S.settings.form.infoDot)}
        aria-describedby={id}
        aria-expanded={open()}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((was) => !was)}
        class="grid size-(--icon-control-h) place-items-center rounded-pill text-faint transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
      >
        <Icon name="info" size={13} />
      </button>
      <span
        id={id}
        role="tooltip"
        class="pointer-events-none absolute bottom-[calc(100%+6px)] left-1/2 z-40 w-64 -translate-x-1/2 rounded-panel border border-line bg-surface px-sm py-2xs text-2xs leading-relaxed text-muted shadow-pop transition-opacity duration-[var(--dur-fast)]"
        classList={{ "opacity-0": !open(), "opacity-100": open() }}
        aria-hidden={!open()}
      >
        {props.text}
      </span>
    </span>
  );
}

/** A section heading inside a page, in the same rhythm as `SettingsView`. */
export function SectionHead(props: {
  title: string;
  desc: string;
  /** The section's icon, carrying meaning the one-line description has to leave out. */
  icon?: IconName;
  /** The long text behind the title, kept in an `InfoDot` rather than spread on the page. */
  more?: string;
  actions?: () => JSX.Element;
}) {
  return (
    <div class="flex flex-wrap items-end justify-between gap-sm">
      <div class="flex min-w-0 flex-1 items-start gap-sm">
        <Show when={props.icon}>
          {(icon) => (
            <span class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
              <Icon name={icon()} size={15} />
            </span>
          )}
        </Show>
        <div class="flex min-w-0 flex-col gap-3xs">
          <h2 class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
            {props.title}
            <Show when={props.more}>{(more) => <InfoDot text={more()} />}</Show>
          </h2>
          <p class="m-0 text-xs text-muted">{props.desc}</p>
        </div>
      </div>
      <Show when={props.actions}>{(render) => <div class="flex gap-sm">{render()()}</div>}</Show>
    </div>
  );
}

/** Dialog primary and secondary buttons: same height and radius, different weight. */
export function Button(props: {
  label: string;
  variant?: "primary" | "ghost" | "outline";
  type?: "button" | "submit";
  icon?: IconName;
  disabled?: boolean;
  busy?: boolean;
  onClick?: () => void;
}) {
  const variant = () => props.variant ?? "primary";
  return (
    <button
      type={props.type ?? "button"}
      disabled={props.disabled || props.busy}
      aria-busy={props.busy}
      onClick={() => props.onClick?.()}
      class="pai-btn text-xs"
      classList={{
        "pai-btn-primary": variant() === "primary",
        "pai-btn-secondary": variant() === "outline",
        "pai-btn-ghost": variant() === "ghost",
      }}
    >
      <Show when={props.icon}>{(icon) => <Icon name={icon()} size={13} />}</Show>
      {props.label}
    </button>
  );
}
