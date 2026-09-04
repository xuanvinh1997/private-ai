import { For, Show, createSignal, onMount } from "solid-js";
import { isDemo } from "../../lib/demo";
import { formatLabel, readDocument } from "../../lib/docs";
import { demoChunks } from "../../lib/fixtures/docs";
import { S, t } from "../../lib/i18n";
import type { DocumentChunkView, DocumentView } from "../../lib/protocol";
import DialogShell, { Button } from "../projects/DialogShell";

/** One page of chunks per fetch. Enough that a short document arrives whole, small enough that a 500-page
 * manual does not paste itself into the DOM before the dialog can paint. */
const PAGE = 40;

/**
 * What the library actually stored for one document.
 *
 * It exists for recordings above all: a PDF can be opened beside the app and compared, an audio file
 * cannot — "did it hear this right" has no answer anywhere else. The same view serves every format,
 * because the question is the same one for a scanned page.
 *
 * The text is quoted, never rendered as Markdown: it is someone else's words, and a heading in it is a
 * heading in *their* document, not a heading in this dialog.
 */
export default function DocumentViewer(props: {
  doc: DocumentView;
  /** Re-extract this one file — for a recording, run speech recognition over it again. */
  onRerun: () => void;
  busy?: boolean;
  onClose: () => void;
}) {
  const [chunks, setChunks] = createSignal<DocumentChunkView[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  /** False once a fetch comes back short: that is the end of the document. */
  const [more, setMore] = createSignal(false);

  const load = async (offset: number) => {
    setLoading(true);
    setError(null);
    try {
      const page = isDemo()
        ? demoChunks(props.doc.id).slice(offset, offset + PAGE)
        : await readDocument(props.doc.id, offset, PAGE);
      setChunks((current) => (offset === 0 ? page : [...current, ...page]));
      setMore(page.length === PAGE);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  onMount(() => void load(0));

  /** The chunker keeps the Markdown heading line inside the chunk it opens, and this dialog already draws
   * that heading above the text. Strip it, or every block starts by repeating its own title. */
  const body = (chunk: DocumentChunkView): string => {
    const [first, ...rest] = chunk.text.split("\n");
    if (first === undefined || !/^#{1,6}\s/.test(first)) return chunk.text;
    return rest.join("\n").trimStart();
  };

  /** Headings repeat across consecutive chunks; show one per run, or a transcript reads as a list of
   * identical timestamps. */
  const headingOf = (index: number): string | null => {
    const heading = chunks()[index]?.heading ?? null;
    if (heading === null || heading === "") return null;
    return index > 0 && chunks()[index - 1]?.heading === heading ? null : heading;
  };

  const empty = () => !loading() && error() === null && chunks().length === 0;

  return (
    <DialogShell
      icon="document"
      width="lg"
      title={props.doc.title}
      desc={t(S.docs.viewer.desc, {
        format: formatLabel(props.doc.format),
        chunks: props.doc.chunks,
      })}
      busy={loading() || props.busy === true}
      onClose={props.onClose}
      footer={() => (
        <>
          <Button
            variant="outline"
            disabled={props.busy === true}
            onClick={() => props.onRerun()}
          >
            {t(S.docs.viewer.rerun)}
          </Button>
          <Button variant="primary" onClick={props.onClose}>
            {t(S.common.close)}
          </Button>
        </>
      )}
    >
      <p class="m-0 truncate font-mono text-2xs text-faint" dir="rtl" title={props.doc.path}>
        <bdi>{props.doc.path}</bdi>
      </p>

      <Show when={props.doc.error}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={error()}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <div
        class="max-h-[52vh] overflow-y-auto rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)"
        aria-live="polite"
        aria-busy={loading() ? "true" : "false"}
      >
        <Show
          when={!empty()}
          fallback={
            <p class="m-0 py-lg text-center text-xs text-muted">{t(S.docs.viewer.empty)}</p>
          }
        >
          <ol class="m-0 flex list-none flex-col gap-sm p-0">
            <For each={chunks()}>
              {(chunk, index) => (
                <li class="flex flex-col gap-3xs">
                  <Show when={headingOf(index())}>
                    {(heading) => (
                      <h3 class="m-0 text-2xs font-semibold tracking-wide text-accent-ink uppercase">
                        {heading()}
                      </h3>
                    )}
                  </Show>
                  <div class="flex gap-sm">
                    <span class="shrink-0 pt-3xs font-mono text-2xs text-faint tabular-nums">
                      {chunk.page > 0
                        ? t(S.docs.viewer.page, { n: chunk.page })
                        : t(S.docs.viewer.ordinal, { n: chunk.ordinal })}
                    </span>
                    {/* Verbatim, and marked as a quotation: this is the document's text, not the app's. */}
                    <blockquote class="m-0 min-w-0 border-l-2 border-line pl-sm text-xs leading-relaxed whitespace-pre-wrap text-ink">
                      {body(chunk)}
                    </blockquote>
                  </div>
                </li>
              )}
            </For>
          </ol>

          <Show when={more()}>
            <div class="flex justify-center pt-sm">
              <Button
                variant="ghost"
                disabled={loading()}
                onClick={() => void load(chunks().length)}
              >
                {loading() ? t(S.common.loading) : t(S.docs.viewer.loadMore)}
              </Button>
            </div>
          </Show>
        </Show>
      </div>
    </DialogShell>
  );
}
