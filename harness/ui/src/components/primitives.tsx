import { createSignal, createUniqueId, Show, type JSX } from "solid-js";
import { useTranscriptActions } from "../lib/transcriptActions";
import { S, t } from "../lib/i18n";
import type { Msg } from "../lib/i18n";
import { notify } from "../lib/toast";
import Icon, { type IconName } from "./Icon";

/** Tool call state; `warn` is for a command that finished with a non-zero exit code. */
export type DotState = "running" | "ok" | "error" | "warn";

export function StateDot(props: { state: DotState; label?: string }) {
  // The label is computed at use site: `t()` reads the locale signal, so a language change updates it in place.
  const label = () => props.label ?? t(stateLabel(props.state));
  return (
    <span
      role="img"
      aria-label={label()}
      title={label()}
      class="size-1.5 shrink-0 rounded-pill"
      classList={{
        // The running dot pulses: a hung tool and a finished one look identical if the dot is still.
        "bg-muted motion-safe:animate-pulse": props.state === "running",
        "bg-success": props.state === "ok",
        "bg-warn": props.state === "warn",
        "bg-danger": props.state === "error",
      }}
    />
  );
}

const LABEL: Record<DotState, Msg> = {
  running: S.tools.state.running,
  ok: S.tools.state.ok,
  warn: S.tools.state.warn,
  error: S.tools.state.error,
};

/** Status text for a dot, shared with the tool card's accessible label. */
export const stateLabel = (state: DotState): Msg => LABEL[state];

type TipSide = "right" | "bottom" | "left";

/** Icon-only button; `aria-label` is required by the signature, since an unlabelled icon button does not exist
 * for a screen reader. The tooltip is an `aria-hidden` span, not `title`, which never shows on keyboard focus. */
export function IconButton(props: {
  icon: IconName;
  label: string;
  onClick?: (event: MouseEvent) => void;
  size?: "sm" | "md" | "lg";
  active?: boolean;
  danger?: boolean;
  disabled?: boolean;
  busy?: boolean;
  expanded?: boolean;
  controls?: string;
  keys?: string;
  tip?: TipSide;
  ref?: (el: HTMLButtonElement) => void;
}) {
  const box = () =>
    props.size === "lg"
      ? "size-(--cta-h)"
      : props.size === "sm"
        ? "size-(--icon-control-h)"
        : "size-(--control-h)";
  const glyph = () => (props.size === "lg" ? 18 : props.size === "sm" ? 13 : 15);
  return (
    <span class="group/tip relative inline-flex shrink-0">
      <button
        ref={props.ref}
        type="button"
        onClick={(event) => props.onClick?.(event)}
        disabled={props.disabled || props.busy}
        aria-label={props.label}
        aria-busy={props.busy || undefined}
        aria-pressed={props.active}
        aria-expanded={props.expanded}
        aria-controls={props.controls}
        aria-keyshortcuts={props.keys}
        class={`grid ${box()} place-items-center rounded-icon border border-transparent transition duration-[var(--dur-fast)]`}
        classList={{
          "text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
            !props.active && !props.danger,
          "bg-accent-soft text-accent-ink": props.active === true,
          "text-danger hover:bg-danger-soft": props.danger === true,
          "cursor-wait opacity-70": props.busy === true,
          "disabled:cursor-not-allowed disabled:opacity-40": props.busy !== true,
        }}
      >
        <Icon
          name={props.icon}
          size={glyph()}
          class={props.busy ? "motion-safe:animate-spin" : undefined}
        />
      </button>
      <Tip side={props.tip ?? "bottom"}>{props.label}</Tip>
    </span>
  );
}

/** Shared tooltip; decoration only, since the real text is the button's `aria-label`. */
function Tip(props: { side: TipSide; children: JSX.Element }) {
  return (
    <span
      aria-hidden="true"
      class="pointer-events-none absolute z-50 hidden rounded-btn bg-ink px-2xs py-3xs text-2xs whitespace-nowrap text-bg opacity-0 shadow-float transition-opacity duration-[var(--dur-fast)] group-hover/tip:opacity-100 group-focus-within/tip:opacity-100 md:block"
      classList={{
        "left-full top-1/2 ml-sm -translate-y-1/2": props.side === "right",
        "right-full top-1/2 mr-sm -translate-y-1/2": props.side === "left",
        "top-full left-1/2 mt-2xs -translate-x-1/2": props.side === "bottom",
      }}
    >
      {props.children}
    </span>
  );
}

/** A collapsible block; `aria-controls` needs a real id, which `createUniqueId` provides per instance. */
export function Disclosure(props: {
  label: string;
  hint?: string;
  open?: boolean;
  children: JSX.Element;
}) {
  const [open, setOpen] = createSignal(props.open ?? false);
  const id = createUniqueId();
  return (
    <div class="flex flex-col">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open()}
        aria-controls={id}
        class="flex min-h-(--icon-control-h) items-center gap-2xs self-start rounded-btn px-xs text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
      >
        <Icon
          name="chevron-right"
          size={12}
          class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
        />
        {props.label}
        <Show when={props.hint}>
          <span class="text-faint">{props.hint}</span>
        </Show>
      </button>
      <div id={id} hidden={!open()} class="mt-2xs">
        {props.children}
      </div>
    </div>
  );
}

/** Copy button; the icon swaps for 1.5s and reverts, with no toast, since a success needs no screen corner. */
export function CopyButton(props: { text: () => string; label?: string }) {
  const [done, setDone] = createSignal(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.text());
      setDone(true);
      setTimeout(() => setDone(false), 1500);
    } catch (err) {
      console.error("copy failed", err);
      notify("error", t(S.tools.copy.failed));
    }
  };
  return (
    <IconButton
      icon={done() ? "check" : "copy"}
      label={done() ? t(S.common.copied) : (props.label ?? t(S.tools.copy.content))}
      size="sm"
      onClick={() => void copy()}
    />
  );
}

/** Long paths are elided in the *middle*, keeping the filename; with a viewer available, the path is a button. */
export function FilePath(props: { path: string; line?: number }) {
  const actions = useTranscriptActions();
  const open = () => actions.openFile;
  return (
    <Show
      when={open()}
      fallback={
        <span class="min-w-0 truncate font-mono text-xs text-accent-ink" dir="rtl" title={props.path}>
          <bdi>{props.path}</bdi>
        </span>
      }
    >
      {(go) => (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            go()(props.path, props.line);
          }}
          title={
            props.line === undefined
              ? t(S.tools.openFile, { path: props.path })
              : t(S.tools.openFileAt, { path: props.path, n: props.line })
          }
          class="min-w-0 truncate rounded-btn font-mono text-xs text-accent-ink underline decoration-transparent underline-offset-2 transition-colors duration-[var(--dur-fast)] hover:decoration-current"
          dir="rtl"
        >
          <bdi>{props.path}</bdi>
        </button>
      )}
    </Show>
  );
}

/** Small label beside a title: model, scope, count. */
export function Chip(props: { children: JSX.Element; tone?: "neutral" | "accent" | "warn" }) {
  return (
    <span
      class="inline-flex shrink-0 items-center gap-3xs rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
      classList={{
        "bg-[var(--overlay-faint)] text-muted": (props.tone ?? "neutral") === "neutral",
        "bg-accent-soft text-accent-ink": props.tone === "accent",
        "bg-warn-soft text-warn": props.tone === "warn",
      }}
    >
      {props.children}
    </span>
  );
}
