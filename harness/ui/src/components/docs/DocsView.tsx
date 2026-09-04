import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { isDemo } from "../../lib/demo";
import {
  addDocuments,
  dismissDocumentTask,
  documentTasks,
  getOcrSetting,
  libraryStats,
  listDocuments,
  reprocessLibrary,
  syncLibrary,
  pickDocuments,
  removeDocument,
  runDocumentTask,
  setOcrEnabled,
  stageLabel,
  type DocumentTask,
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
  const [error, setError] = createSignal<string | null>(null);
  const [removing, setRemoving] = createSignal<DocumentView | null>(null);
  const [actionBusy, setActionBusy] = createSignal(false);
  const [ocr, setOcr] = createSignal<OcrSetting | null>(null);
  const [ocrBusy, setOcrBusy] = createSignal(false);
  const task = createMemo(() => documentTasks()[props.resetKey] ?? null);
  const busy = () => actionBusy() || task()?.state === "running";

  let appliedTaskId = "";

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

    // Then sync with the folder. A task already running survives screen navigation and must not be duplicated.
    if (task()?.state === "running") return;
    try {
      const root = health.root;
      await runDocumentTask(
        props.resetKey,
        "sync",
        progress(root, 0),
        (note) => syncLibrary(note),
      );
    } catch (err) {
      // A failed scan must not erase the list: what was ingested before is still searchable.
      console.error(t(S.docs.error.scan, { err: String(err) }));
    }
  };

  // Reload on project change, not `onMount`: this component outlives one project.
  createEffect(
    on(
      () => props.resetKey,
      () => {
        setDocs([]);
        setStats(null);
        setError(null);
        setOcr(null);
        appliedTaskId = "";
        void load();
      },
    ),
  );

  // A task can finish while this screen is unmounted. Apply its result when the project view returns.
  createEffect(() => {
    const current = task();
    if (current?.documents === null || current === null || current.id === appliedTaskId) return;
    appliedTaskId = current.id;
    setDocs(current.documents);
    if (!isDemo()) void libraryStats().then(setStats);
  });

  async function runDemoIngest(
    paths: string[],
    note: (frame: IngestProgress) => void,
  ): Promise<DocumentView[]> {
    for (const frame of demoIngestFrames(paths)) {
      await new Promise<void>((resolve) => setTimeout(resolve, 320));
      note(frame);
    }
    setStats(demoLibraryStats());
    return demoDocuments();
  }

  const addFiles = async (paths: string[]) => {
    if (paths.length === 0 || busy()) return;
    setError(null);
    try {
      await runDocumentTask(
        props.resetKey,
        "add",
        progress(paths[0] ?? "", paths.length),
        (note) => (isDemo() ? runDemoIngest(paths, note) : addDocuments(paths, note)),
      );
    } catch (err) {
      console.error("document ingest failed", err);
    }
  };

  /** Reprocess the whole library through the same progress path as ingest, file by file. */
  const reprocess = async () => {
    if (busy()) return;
    setError(null);
    try {
      const paths = demoDocuments().map((doc) => doc.path);
      await runDocumentTask(
        props.resetKey,
        "reprocess",
        progress(stats()?.root ?? "", 0),
        (note) => (isDemo() ? runDemoIngest(paths, note) : reprocessLibrary(note)),
      );
    } catch (err) {
      console.error(t(S.docs.error.reprocess, { err: String(err) }));
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
    setActionBusy(true);
    setError(null);
    try {
      if (!isDemo()) await removeDocument(doc.id);
      setDocs((all) => all.filter((entry) => entry.id !== doc.id));
      if (!isDemo()) setStats(await libraryStats());
    } catch (err) {
      setError(t(S.docs.error.remove, { title: doc.title, err: String(err) }));
    } finally {
      setActionBusy(false);
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

          <Show when={task()}>
            {(current) => (
              <DocumentTaskCard
                task={current()}
                onDismiss={() => dismissDocumentTask(props.resetKey)}
              />
            )}
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <Show when={!loading() && (docs().length > 0 || hasInlineFileProgress(task()))}>
            <DocumentTable
              docs={docs()}
              busy={busy()}
              task={task()}
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
          <SearchProbe disabled={actionBusy()} />
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

function progress(path: string, total: number): IngestProgress {
  return {
    path,
    stage: "preparing",
    done: 0,
    total,
    finished: false,
    error: null,
  };
}

/** Persistent task card: determinate for known work, indeterminate while scanning or probing a model. */
function DocumentTaskCard(props: { task: DocumentTask; onDismiss: () => void }) {
  const [clock, setClock] = createSignal(Date.now());
  const timer = window.setInterval(() => setClock(Date.now()), 1_000);
  onCleanup(() => window.clearInterval(timer));

  const running = () => props.task.state === "running";
  const frame = () => props.task.progress;
  const determinate = () => frame().total > 0;
  const fileProgress = () => hasInlineFileProgress(props.task);
  const percentage = () =>
    determinate() ? Math.min(100, Math.round((frame().done / frame().total) * 100)) : 0;
  const elapsed = () =>
    formatDuration((props.task.finishedAt ?? clock()) - props.task.startedAt);
  const count = () =>
    frame().stage === "embedding"
      ? t(S.docs.ingest.chunks, { done: frame().done, total: frame().total })
      : frame().stage === "ocr"
        ? t(S.docs.ingest.pages, { done: frame().done, total: frame().total })
        : t(S.docs.ingest.files, { done: frame().done, total: frame().total });
  const stateLabel = () =>
    props.task.state === "completed"
      ? t(S.docs.ingest.statusCompleted)
      : props.task.state === "failed"
        ? t(S.docs.ingest.statusFailed)
        : stageLabel(frame().stage);
  const kindLabel = () =>
    props.task.kind === "sync"
      ? t(S.docs.ingest.kindSync)
      : props.task.kind === "add"
        ? t(S.docs.ingest.kindAdd)
        : t(S.docs.ingest.kindReprocess);
  const icon = () =>
    props.task.state === "completed" ? "check" : props.task.state === "failed" ? "warn" : "clock";

  return (
    <div class="flex flex-col gap-sm rounded-card border border-line bg-surface-soft px-(--card-pad-x) py-(--card-pad-y)">
      <span class="sr-only" role="status" aria-live="polite" aria-atomic="true">
        {kindLabel()}, {stateLabel()}
        <Show when={determinate()}>, {count()}</Show>
      </span>
      <div class="flex items-start gap-sm">
        <span
          class={`mt-3xs grid size-6 shrink-0 place-items-center rounded-panel ${
            props.task.state === "failed"
              ? "bg-danger-soft text-danger"
              : props.task.state === "completed"
                ? "bg-success-soft text-success"
                : "bg-accent-soft text-accent-ink"
          }`}
        >
          <Icon name={icon()} size={13} />
        </span>
        <div class="flex min-w-0 flex-1 flex-col gap-3xs">
          <div class="flex min-w-0 items-baseline justify-between gap-sm">
            <p class="m-0 min-w-0 truncate text-xs font-medium text-ink">
              {kindLabel()}
              <span class="font-normal text-muted"> · {stateLabel()}</span>
            </p>
            <span class="shrink-0 text-2xs text-muted tabular-nums">
              {t(S.docs.ingest.elapsed, { time: elapsed() })}
            </span>
          </div>
          <Show when={running()}>
            <p class="m-0 truncate text-2xs text-muted" title={frame().path}>
              {stageLabel(frame().stage)}
              <Show when={frame().path}> · <span class="font-mono">{fileName(frame().path)}</span></Show>
            </p>
          </Show>
        </div>
        <Show when={!running()}>
          <button
            type="button"
            onClick={props.onDismiss}
            aria-label={t(S.docs.ingest.dismiss)}
            class="grid size-6 shrink-0 place-items-center rounded-icon text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
          >
            <Icon name="x" size={13} />
          </button>
        </Show>
      </div>

      <Show when={running() && !fileProgress()}>
        <div class="flex flex-col gap-2xs">
          <div class="flex items-center justify-between gap-sm text-2xs text-muted tabular-nums">
            <span>{determinate() ? count() : t(S.docs.ingest.background)}</span>
            <Show when={determinate()}>
              <span>{percentage()}%</span>
            </Show>
          </div>
          <div
            role="progressbar"
            aria-valuenow={determinate() ? frame().done : undefined}
            aria-valuemin={determinate() ? 0 : undefined}
            aria-valuemax={determinate() ? frame().total : undefined}
            aria-valuetext={`${stageLabel(frame().stage)}${determinate() ? `, ${count()}` : ""}`}
            aria-label={t(S.docs.ingest.progressLabel)}
            class="h-1.5 overflow-hidden rounded-pill bg-[var(--overlay-faint)]"
          >
            <div
              class={`h-full w-full origin-left rounded-pill bg-accent transition-transform duration-[var(--dur-base)] motion-reduce:transition-none ${
                determinate() ? "" : "animate-pulse motion-reduce:animate-none"
              }`}
              style={{ transform: `scaleX(${determinate() ? percentage() / 100 : 0.35})` }}
            />
          </div>
        </div>
      </Show>

      <Show when={!running()}>
        <p class="m-0 text-2xs text-muted">
          {t(S.docs.ingest.summary, {
            stored: props.task.stored,
            skipped: props.task.skipped,
            failed: props.task.failures.length,
          })}
        </p>
      </Show>

      <Show when={props.task.warning}>
        {(warning) => (
          <p class="m-0 rounded-panel bg-warn-soft px-sm py-2xs text-2xs break-words text-text">
            <strong class="font-medium">{t(S.docs.ingest.warning)}:</strong> {warning()}
          </p>
        )}
      </Show>
      <Show when={props.task.error}>
        {(message) => (
          <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-2xs break-words text-danger" role="alert">
            {message()}
          </p>
        )}
      </Show>
      <Show when={props.task.failures.length > 0}>
        <ul class="m-0 flex list-none flex-col gap-2xs border-t border-line p-0 pt-sm">
          <For each={props.task.failures.slice(0, 5)}>
            {(failure) => (
              <li class="flex min-w-0 items-baseline gap-sm text-2xs">
                <span class="min-w-0 flex-1 truncate font-mono text-text" title={failure.path}>
                  {fileName(failure.path)}
                </span>
                <span class="max-w-[55%] truncate text-danger" title={failure.error}>
                  {failure.error}
                </span>
              </li>
            )}
          </For>
          <Show when={props.task.failures.length > 5}>
            <li class="text-2xs text-muted">
              {t(S.docs.ingest.moreFailures, { count: props.task.failures.length - 5 })}
            </li>
          </Show>
        </ul>
      </Show>
    </div>
  );
}

function hasInlineFileProgress(task: DocumentTask | null): boolean {
  if (task?.state !== "running" || task.progress.total === 0) return false;
  return ["reading", "ocr", "stored", "failed", "skipped"].includes(task.progress.stage);
}

function formatDuration(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}m ${rest}s`;
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
