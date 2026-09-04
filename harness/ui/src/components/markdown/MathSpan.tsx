import { createEffect, createResource, createSignal, Show } from "solid-js";
import { S, t } from "../../lib/i18n";
import { loadKatex, renderMath } from "../../lib/katex";

/** One formula, rendered by KaTeX into a Solid-owned node; loading, broken and unavailable all still show the TeX source, because a silently vanished formula takes the answer with it. */
export default function MathSpan(props: { tex: string; display: boolean }) {
  const [katex] = createResource(loadKatex);
  const [error, setError] = createSignal<string | null>(null);
  let host: HTMLElement | undefined;

  createEffect(() => {
    const mod = katex();
    // Read both props before any early return, or the effect stops tracking them.
    const tex = props.tex;
    const display = props.display;
    if (mod === undefined || host === undefined) return;
    setError(renderMath(mod, host, tex, display));
  });

  return (
    <Show
      when={katex.error === undefined && error() === null}
      fallback={<Fallback tex={props.tex} display={props.display} message={error()} />}
    >
      {/* `display` is its own scrolling block, so a wide matrix cannot stretch the transcript. */}
      <Show
        when={props.display}
        fallback={<span ref={(el) => (host = el)}>{props.tex}</span>}
      >
        <div class="overflow-x-auto py-2xs text-center" ref={(el) => (host = el)}>
          {props.tex}
        </div>
      </Show>
    </Show>
  );
}

/** When rendering is not possible, show the source as code, with the reason when there is one. */
function Fallback(props: { tex: string; display: boolean; message: string | null }) {
  const code = (
    <code class="rounded-btn bg-[var(--overlay-faint)] px-3xs py-px font-mono text-2xs text-text">
      {props.tex}
    </code>
  );
  return (
    <Show when={props.display} fallback={code}>
      <div class="flex flex-col gap-3xs overflow-x-auto py-2xs">
        {code}
        <Show when={props.message}>
          {(message) => (
            <span class="text-2xs text-danger">
              {t(S.tools.renderFailed)} {message()}
            </span>
          )}
        </Show>
      </div>
    </Show>
  );
}
