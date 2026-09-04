import { Show } from "solid-js";
import { S, t } from "../../lib/i18n";
import { langLabel } from "./fences";
import { CopyButton } from "../primitives";

/** A fenced code block in any language, framed exactly like `DiffBlock` so the two read alike. */
export default function CodeFence(props: { lang: string; code: string; streaming?: boolean }) {
  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="min-w-0 truncate text-2xs text-muted">
          {langLabel(props.lang)}
          <Show when={props.streaming}>
            <span class="text-faint"> · {t(S.tools.code.streaming)}</span>
          </Show>
        </figcaption>
        <CopyButton text={() => props.code} label={t(S.tools.code.copy)} />
      </div>

      {/* Scrolls inside its own frame, so a long line cannot widen the transcript. */}
      <div class="overflow-x-auto" aria-busy={props.streaming === true}>
        <pre class="m-0 w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] text-text">
          {props.code === "" ? " " : props.code}
        </pre>
      </div>
    </figure>
  );
}
