import { createMemo, createResource, createSignal, Match, Switch } from "solid-js";
import { S, t } from "../../lib/i18n";
import { diagramKind, isDark, renderDiagram } from "../../lib/mermaid";
import { CopyButton } from "../primitives";

type Mode = "figure" | "source";

/** A mermaid diagram card with two ways in, figure and source, because a mermaid SVG reads as scattered labels to a screen reader and the source view is the honest alternative. */
export default function Diagram(props: { source: string }) {
  const [mode, setMode] = createSignal<Mode>("figure");
  const kind = createMemo(() => diagramKind(props.source));

  // `isDark()` is read in the resource source: mermaid bakes colours into the SVG, so a theme change redraws.
  const [result] = createResource(
    () => ({ source: props.source, dark: isDark() }),
    (input) => renderDiagram(input.source),
  );

  const failure = createMemo(() => {
    const value = result();
    return value !== undefined && !value.ok ? value.message : null;
  });
  const svg = createMemo(() => {
    const value = result();
    return value !== undefined && value.ok ? value.svg : null;
  });

  return (
    <figure class="m-0 overflow-hidden rounded-panel border border-line bg-surface">
      <div class="flex items-center justify-between gap-sm border-b border-line px-sm py-3xs">
        <figcaption class="min-w-0 truncate text-2xs text-muted">
          {t(S.tools.diagram.title)} · {kind()}
        </figcaption>
        <div class="flex items-center gap-3xs">
          <div
            role="group"
            aria-label={t(S.tools.diagram.views)}
            class="flex items-center gap-3xs"
          >
            <ModeButton
              label={t(S.tools.diagram.figure)}
              active={mode() === "figure"}
              onPick={() => setMode("figure")}
            />
            <ModeButton
              label={t(S.tools.diagram.source)}
              active={mode() === "source"}
              onPick={() => setMode("source")}
            />
          </div>
          <CopyButton text={() => props.source} label={t(S.tools.diagram.copySource)} />
        </div>
      </div>

      <Switch>
        {/* Broken syntax shows both the message and the source, whichever view is active. */}
        <Match when={failure()}>
          {(message) => (
            <div class="flex flex-col gap-2xs px-sm py-2xs">
              <p class="m-0 flex items-start gap-2xs text-2xs text-danger">
                <span class="shrink-0">{t(S.tools.renderFailed)}</span>
                <span class="min-w-0 whitespace-pre-wrap">{message()}</span>
              </p>
              <Source code={props.source} />
            </div>
          )}
        </Match>

        <Match when={mode() === "source"}>
          <div class="px-sm py-2xs">
            <Source code={props.source} />
          </div>
        </Match>

        <Match when={result.loading}>
          <p class="m-0 px-sm py-md text-2xs text-faint" aria-busy="true">
            {t(S.tools.diagram.drawing)}
          </p>
        </Match>

        <Match when={svg()}>
          {(markup) => (
            <div
              role="img"
              aria-label={t(S.tools.diagram.alt, {
                kind: kind(),
                source: t(S.tools.diagram.source),
              })}
              /* Scrolls here and the SVG shrinks to the frame, so a wide diagram cannot widen the transcript. */
              class="overflow-x-auto px-sm py-sm [&_svg]:h-auto [&_svg]:max-w-full"
              innerHTML={markup()}
            />
          )}
        </Match>
      </Switch>
    </figure>
  );
}

function ModeButton(props: { label: string; active: boolean; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={props.onPick}
      aria-pressed={props.active}
      class="rounded-btn px-2xs py-3xs text-2xs transition-colors duration-[var(--dur-fast)]"
      classList={{
        "text-muted hover:bg-[var(--overlay-hover)] hover:text-ink": !props.active,
        "bg-accent-soft text-accent-ink": props.active,
      }}
    >
      {props.label}
    </button>
  );
}

function Source(props: { code: string }) {
  return (
    <div class="overflow-x-auto rounded-panel bg-surface-soft">
      <pre class="m-0 w-max min-w-full px-sm py-2xs font-mono text-2xs leading-[1.55] text-text">
        {props.code}
      </pre>
    </div>
  );
}
