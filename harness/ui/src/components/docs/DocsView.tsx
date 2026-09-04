import { createEffect, createSignal, For, on, Show } from "solid-js";
import { isDemo } from "../../lib/demo";
import {
  addDocuments,
  getOcrSetting,
  libraryStats,
  listDocuments,
  reprocessLibrary,
  syncLibrary,
  pickDocuments,
  removeDocument,
  setOcrEnabled,
  stageLabel,
} from "../../lib/docs";
import { demoDocuments, demoIngestFrames, demoLibraryStats } from "../../lib/fixtures/docs";
import { S, t, tn } from "../../lib/i18n";
import type { DocumentView, IngestProgress, LibraryStats, OcrSetting } from "../../lib/protocol";
import Icon from "../Icon";
import { InfoDot } from "../settings/FormKit";
import ConfirmDialog from "../projects/ConfirmDialog";
import { Button } from "../projects/DialogShell";
import DocumentTable from "./DocumentTable";
import DropZone from "./DropZone";
import SearchProbe from "./SearchProbe";

interface Failure {
  path: string;
  error: string;
}

/** Document library screen for a `docs` project: one bad file never fails the batch, and semantic search being unready still leaves keyword search working. */
export default function DocsView(props: {
  /** A new value means a new project: drop all state and reload from scratch. */
  resetKey: string;
  /** Library name for the heading; absent falls back to the generic title. */
  name?: string;
}) {
  const [docs, setDocs] = createSignal<DocumentView[]>([]);
  const [stats, setStats] = createSignal<LibraryStats | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [ingest, setIngest] = createSignal<IngestProgress | null>(null);
  const [failures, setFailures] = createSignal<Failure[]>([]);
  const [added, setAdded] = createSignal(0);
  const [error, setError] = createSignal<string | null>(null);
  const [removing, setRemoving] = createSignal<DocumentView | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [ocr, setOcr] = createSignal<OcrSetting | null>(null);
  const [ocrBusy, setOcrBusy] = createSignal(false);

  const load = async () => {
    setLoading(true);
    if (isDemo()) {
      setDocs(demoDocuments());
      setStats(demoLibraryStats());
      setOcr({ enabled: true, visionModel: "qwen2.5vl" });
      setLoading(false);
      return;
    }
    // Show what is known first, then scan, or the screen stays blank through a large folder.
    const [list, health, ocrSetting] = await Promise.all([
      listDocuments(),
      libraryStats(),
      getOcrSetting(),
    ]);
    setDocs(list);
    setStats(health);
    setOcr(ocrSetting);
    setLoading(false);

    // Then sync with the folder: the project folder is the library, so opening it scans.
    try {
      const sau = await syncLibrary(note);
      setDocs(sau);
      setStats(await libraryStats());
    } catch (err) {
      // A failed scan must not erase the list: what was ingested before is still searchable.
      setError(t(S.docs.error.scan, { err: String(err) }));
    } finally {
      setIngest(null);
    }
  };

  // Reload on project change, not `onMount`: this component outlives one project.
  createEffect(
    on(
      () => props.resetKey,
      () => {
        setDocs([]);
        setStats(null);
        setIngest(null);
        setFailures([]);
        setAdded(0);
        setError(null);
        setOcr(null);
        void load();
      },
    ),
  );

  const note = (frame: IngestProgress) => {
    setIngest(frame);
    const reason = frame.error;
    if (reason === null) return;
    // A failed embedding pass is not a failed file: the files landed, the embedder did not answer.
    if (frame.stage === "embedding") {
      setError(reason);
      return;
    }
    setFailures((all) => [...all, { path: frame.path, error: reason }]);
  };

  async function runDemoIngest(paths: string[]): Promise<DocumentView[]> {
    for (const frame of demoIngestFrames(paths)) {
      await new Promise<void>((resolve) => setTimeout(resolve, 320));
      note(frame);
    }
    setStats(demoLibraryStats());
    return demoDocuments();
  }

  const addFiles = async (paths: string[]) => {
    if (paths.length === 0 || busy()) return;
    setBusy(true);
    setError(null);
    setFailures([]);
    setAdded(0);
    setIngest({
      path: paths[0] ?? "",
      stage: "preparing",
      done: 0,
      total: paths.length,
      finished: false,
      error: null,
    });
    try {
      const next = isDemo() ? await runDemoIngest(paths) : await addDocuments(paths, note);
      setDocs(next);
      setAdded(paths.length - failures().length);
      if (!isDemo()) setStats(await libraryStats());
    } catch (err) {
      // Only reached when the whole batch fails; single bad files come through `note`.
      setError(String(err));
    } finally {
      setIngest(null);
      setBusy(false);
    }
  };

  /** Reprocess the whole library through the same progress path as ingest, file by file. */
  const reprocess = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    setFailures([]);
    setAdded(0);
    setIngest({
      path: stats()?.root ?? "",
      stage: "preparing",
      done: 0,
      total: 0,
      finished: false,
      error: null,
    });
    try {
      if (isDemo()) {
        await runDemoIngest(demoDocuments().map((doc) => doc.path));
        setDocs(demoDocuments());
      } else {
        setDocs(await reprocessLibrary(note));
        setStats(await libraryStats());
      }
    } catch (err) {
      setError(t(S.docs.error.reprocess, { err: String(err) }));
    } finally {
      setIngest(null);
      setBusy(false);
    }
  };

  const pick = async () => {
    setError(null);
    try {
      await addFiles(await pickDocuments());
    } catch (err) {
      setError(t(S.docs.error.pick, { err: String(err) }));
    }
  };

  const toggleOcr = async (enabled: boolean) => {
    const previous = ocr();
    if (previous === null || ocrBusy()) return;
    setOcrBusy(true);
    setOcr({ ...previous, enabled });
    try {
      if (!isDemo()) setOcr(await setOcrEnabled(enabled));
    } catch (err) {
      setOcr(previous);
      setError(t(S.docs.error.ocr, { err: String(err) }));
    } finally {
      setOcrBusy(false);
    }
  };

  const confirmRemove = async (doc: DocumentView) => {
    setRemoving(null);
    setBusy(true);
    setError(null);
    try {
      if (!isDemo()) await removeDocument(doc.id);
      setDocs((all) => all.filter((entry) => entry.id !== doc.id));
      if (!isDemo()) setStats(await libraryStats());
    } catch (err) {
      setError(t(S.docs.error.remove, { title: doc.title, err: String(err) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-[880px] flex-col gap-2xl">
        <section class="flex flex-col gap-md">
          <div class="flex items-start gap-sm">
            <span class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
              <Icon name="library" size={15} />
            </span>
            <div class="flex min-w-0 flex-col gap-3xs">
              <h2 class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
                {props.name ?? t(S.docs.title)}
                <InfoDot text={t(S.docs.titleMore)} />
              </h2>
              <p class="m-0 text-xs text-muted">{t(S.docs.subtitle)}</p>
            </div>
          </div>

          <StatsStrip
            stats={stats()}
            loading={loading()}
            busy={busy()}
            onReprocess={() => void reprocess()}
          />
        </section>

        <section class="flex flex-col gap-md">
          <DropZone
            compact={docs().length > 0}
            busy={busy()}
            onPaths={(paths) => void addFiles(paths)}
            onPick={() => void pick()}
          />

          <Show when={ocr()}>
            {(setting) => (
              <label class="flex items-start gap-sm rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) text-xs text-text">
                <input
                  type="checkbox"
                  checked={setting().enabled}
                  disabled={ocrBusy() || busy()}
                  onChange={(event) => void toggleOcr(event.currentTarget.checked)}
                  class="mt-3xs size-4 shrink-0 accent-[var(--accent)]"
                />
                <span class="flex min-w-0 flex-col gap-3xs">
                  <span class="font-medium text-ink">{t(S.docs.ocr.enable)}</span>
                  <span class="text-2xs text-muted">
                    <Show
                      when={setting().visionModel}
                      fallback={<>{t(S.docs.ocr.noModel)}</>}
                    >
                      {(model) => t(S.docs.ocr.ready, { model: model() })}
                    </Show>
                  </span>
                </span>
              </label>
            )}
          </Show>

          <Show when={ingest()}>
            {(frame) => (
              <div
                class="flex flex-col gap-2xs rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)"
                role="status"
                aria-live="polite"
              >
                <div class="flex items-baseline justify-between gap-sm">
                  <span class="min-w-0 truncate text-xs text-text">
                    {stageLabel(frame().stage)}: <span class="font-mono text-2xs">{fileName(frame().path)}</span>
                  </span>
                  <span class="shrink-0 text-2xs text-muted tabular-nums">
                    {frame().done}/{frame().total}
                  </span>
                </div>
                <div
                  role="progressbar"
                  aria-valuenow={frame().done}
                  aria-valuemin={0}
                  aria-valuemax={frame().total}
                  aria-label={t(S.docs.ingest.progressLabel)}
                  class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
                >
                  <div
                    class="h-full rounded-pill bg-accent transition-[width] duration-[var(--dur-base)]"
                    style={{
                      width: `${frame().total === 0 ? 0 : Math.round((frame().done / frame().total) * 100)}%`,
                    }}
                  />
                </div>
              </div>
            )}
          </Show>

          {/* Failed files stand apart from a batch error, and the first line counts what landed. */}
          <Show when={failures().length > 0}>
            <div class="flex flex-col gap-2xs rounded-card border border-line bg-warn-soft px-(--card-pad-x) py-(--card-pad-y)">
              <div class="flex items-start gap-sm">
                <span class="mt-3xs shrink-0 text-warn">
                  <Icon name="warn" size={15} />
                </span>
                <p class="m-0 flex flex-1 flex-wrap items-center gap-2xs text-xs text-text">
                  <span>
                    <Show when={added() > 0} fallback={<>{t(S.docs.failures.none)}</>}>
                      {t(S.docs.failures.some, { ok: added(), bad: failures().length })}
                    </Show>
                  </span>
                  <InfoDot text={t(S.docs.failures.more)} />
                </p>
                <button
                  type="button"
                  onClick={() => setFailures([])}
                  aria-label={t(S.docs.failures.dismiss)}
                  class="shrink-0 rounded-icon p-3xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
                >
                  <Icon name="x" size={13} />
                </button>
              </div>
              <ul class="m-0 flex list-none flex-col gap-2xs p-0 pl-lg">
                <For each={failures()}>
                  {(failure) => (
                    <li class="flex flex-col gap-3xs">
                      <span class="min-w-0 truncate font-mono text-2xs text-text" title={failure.path}>
                        {fileName(failure.path)}
                      </span>
                      <span class="text-2xs text-muted">{failure.error}</span>
                    </li>
                  )}
                </For>
              </ul>
            </div>
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <Show when={!loading() && docs().length > 0}>
            <DocumentTable
              docs={docs()}
              busy={busy()}
              onRemove={(doc) => setRemoving(doc)}
            />
          </Show>

          <Show when={loading()}>
            <p class="m-0 text-xs text-muted" role="status" aria-live="polite">
              {t(S.docs.loadingDocs)}
            </p>
          </Show>
        </section>

        <Show when={docs().length > 0}>
          <SearchProbe disabled={busy()} />
        </Show>
      </div>

      <Show when={removing()}>
        {(doc) => (
          <ConfirmDialog
            icon="trash"
            title={t(S.docs.remove.title, { title: doc().title })}
            body={t(S.docs.remove.body)}
            more={t(S.docs.remove.more)}
            detail={doc().path}
            confirmLabel={t(S.docs.remove.confirm)}
            onClose={() => setRemoving(null)}
            onConfirm={() => void confirmRemove(doc())}
          />
        )}
      </Show>
    </div>
  );
}

/** Library health strip; when semantic search is not ready, say that keyword search still works. */
function StatsStrip(props: {
  stats: LibraryStats | null;
  loading: boolean;
  busy: boolean;
  onReprocess: () => void;
}) {
  return (
    <Show
      when={props.stats}
      fallback={
        <p class="m-0 rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) text-xs text-muted">
          {props.loading ? t(S.docs.stats.loading) : t(S.docs.stats.unknown)}
        </p>
      }
    >
      {(stats) => (
        <div class="flex flex-col gap-sm rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y)">
          <dl class="m-0 flex flex-wrap gap-x-2xl gap-y-sm">
            <Stat label={t(S.docs.stats.documents)} value={String(stats().documents)} />
            <Stat label={t(S.docs.stats.chunks)} value={String(stats().chunks)} />
            <Stat
              label={t(S.docs.stats.embedded)}
              value={`${stats().embeddedChunks}/${stats().chunks}`}
            />
            <Stat
              label={t(S.docs.stats.embedder)}
              value={stats().embedder ?? t(S.docs.stats.embedderNone)}
            />
          </dl>

          <Show
            when={!stats().semanticReady}
            fallback={
              <p class="m-0 flex items-center gap-2xs text-2xs text-success">
                <Icon name="check" size={12} />
                {t(S.docs.stats.ready)}
              </p>
            }
          >
            <div class="flex items-start gap-sm rounded-panel bg-warn-soft px-sm py-2xs">
              <span class="mt-3xs shrink-0 text-warn">
                <Icon name="clock" size={13} />
              </span>
              <p class="m-0 flex flex-wrap items-center gap-2xs text-2xs text-text">
                <span>
                  <Show when={stats().reason}>
                    {(reason) => <>{reason()} </>}
                  </Show>
                  <strong class="font-medium">{t(S.docs.stats.keywordOn)}</strong>{" "}
                  {t(S.docs.stats.keywordOnTail)}
                </span>
                <InfoDot text={t(S.docs.stats.keywordOnMore)} />
              </p>
            </div>
          </Show>

          {/* Always present, even when healthy: a button that only appears on failure is one to hunt for. */}
          <div class="flex flex-wrap items-center justify-between gap-sm border-t border-line pt-sm">
            <p class="m-0 flex max-w-[52ch] flex-wrap items-center gap-2xs text-2xs text-muted">
              <span>
                <Show
                  when={stats().chunks > stats().embeddedChunks}
                  fallback={<>{t(S.docs.reprocess.hint)}</>}
                >
                  {tn(
                    stats().chunks - stats().embeddedChunks,
                    S.docs.reprocess.pendingOne,
                    S.docs.reprocess.pendingOther,
                  )}
                </Show>
              </span>
              <InfoDot text={t(S.docs.reprocess.more)} />
            </p>
            <Button
              variant="outline"
              icon="retry"
              disabled={props.busy}
              onClick={props.onReprocess}
            >
              {props.busy ? t(S.docs.reprocess.busy) : t(S.docs.reprocess.action)}
            </Button>
          </div>
        </div>
      )}
    </Show>
  );
}

function Stat(props: { label: string; value: string }) {
  return (
    <div class="flex flex-col gap-3xs">
      <dt class="m-0 text-2xs text-faint">{props.label}</dt>
      <dd class="m-0 text-sm text-ink tabular-nums">{props.value}</dd>
    </div>
  );
}

/** File name for the progress row: a full path pushes the done/total counter off screen. */
function fileName(path: string): string {
  return path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;
}
