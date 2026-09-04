import { For, Show, createSignal } from "solid-js";
import { isDemo } from "../../lib/demo";
import { searchDocuments } from "../../lib/docs";
import { S, t } from "../../lib/i18n";
import { demoHits } from "../../lib/fixtures/docs";
import type { DocumentHit } from "../../lib/protocol";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import { Button } from "../projects/DialogShell";

/** Search probe: check the library before asking the assistant, so retrieval and reasoning faults stay apart; excerpts are quoted verbatim, since they are someone else's words. */
export default function SearchProbe(props: { disabled?: boolean }) {
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<DocumentHit[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const run = async () => {
    const text = query().trim();
    if (text === "" || busy()) return;
    setBusy(true);
    setError(null);
    try {
      setHits(isDemo() ? demoHits(text) : await searchDocuments(text));
    } catch (err) {
      setHits(null);
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="flex flex-col gap-md">
      <div class="flex flex-col gap-3xs">
        <h3 class="m-0 flex items-center gap-2xs text-sm font-semibold text-ink">
          {t(S.docs.probe.title)}
          <InfoDot text={t(S.docs.probe.titleMore)} />
        </h3>
        <p class="m-0 text-xs text-muted">
          {t(S.docs.probe.subtitle)}
        </p>
      </div>

      <div class="flex flex-wrap gap-sm">
        <label class="flex min-w-[220px] flex-1 items-center gap-2xs rounded-btn border border-line-strong bg-surface px-sm transition-colors duration-[var(--dur-fast)] focus-within:border-accent">
          <span class="shrink-0 text-faint">
            <Icon name="search" size={14} />
          </span>
          <input
            type="search"
            value={query()}
            spellcheck={false}
            placeholder={t(S.docs.probe.placeholder)}
            aria-label={t(S.docs.probe.inputLabel)}
            disabled={props.disabled}
            onInput={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void run();
              }
            }}
            class="h-(--control-h) min-w-0 flex-1 bg-transparent text-xs text-text outline-none placeholder:text-faint disabled:opacity-50"
          />
        </label>
        <Button
          variant="outline"
          disabled={props.disabled || busy() || query().trim() === ""}
          onClick={() => void run()}
        >
          {busy() ? t(S.docs.probe.busy) : t(S.common.search)}
        </Button>
      </div>

      <Show when={error()}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <div aria-live="polite" aria-busy={busy() ? "true" : "false"}>
        <Show when={hits()}>
          {(list) => (
            <Show
              when={list().length > 0}
              fallback={
                <p class="m-0 flex items-center justify-center gap-2xs rounded-card border border-dashed border-line px-(--card-pad-x) py-lg text-center text-xs text-muted">
                  {t(S.docs.probe.empty)}
                  <InfoDot text={t(S.docs.probe.emptyMore)} />
                </p>
              }
            >
              <ul class="m-0 flex list-none flex-col gap-sm p-0">
                <For each={list()}>{(hit) => <Hit hit={hit} />}</For>
              </ul>
            </Show>
          )}
        </Show>
      </div>
    </section>
  );
}

function Hit(props: { hit: DocumentHit }) {
  return (
    <li class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
      <div class="flex flex-wrap items-center gap-2xs">
        <span class="min-w-0 truncate text-xs font-medium text-ink" title={props.hit.path}>
          {props.hit.title}
        </span>
        <span class="text-2xs text-faint tabular-nums">
          {t(S.docs.probe.ordinal, { n: props.hit.ordinal })}
        </span>
        <MatchBadge by={props.hit.matchedBy} />
        <span class="ml-auto text-2xs text-faint tabular-nums">
          {props.hit.score.toFixed(2)}
        </span>
      </div>

      {/* Rule and italics mark a quotation: never trimmed or corrected, these are the document's words. */}
      <blockquote class="m-0 border-l-2 border-line-strong bg-surface-soft py-2xs pr-sm pl-md text-xs whitespace-pre-wrap text-text italic">
        {props.hit.text}
      </blockquote>
      <p class="m-0 text-2xs text-faint">{t(S.docs.probe.quoteNote)}</p>
    </li>
  );
}

/** Why this chunk matched: a library still embedding returns only `keyword`, which the score cannot show. */
function MatchBadge(props: { by: DocumentHit["matchedBy"] }) {
  const label = () =>
    props.by === "both"
      ? t(S.docs.probe.matchBoth)
      : props.by === "semantic"
        ? t(S.docs.probe.matchSemantic)
        : t(S.docs.probe.matchKeyword);
  return (
    <span
      class="inline-flex shrink-0 items-center rounded-pill px-2xs py-3xs text-2xs whitespace-nowrap"
      classList={{
        "bg-accent-soft text-accent-ink": props.by === "both",
        "bg-[var(--overlay-faint)] text-muted": props.by !== "both",
      }}
    >
      {label()}
    </span>
  );
}
