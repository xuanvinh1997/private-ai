import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { createSignal } from "solid-js";
import { inTauri } from "./agent";
import { S, t, type Msg } from "./i18n";
import { dismissAppNotification, upsertAppNotification } from "./notifications";
import type {
  DocumentFormat,
  DocumentHit,
  DocumentView,
  IngestProgress,
  LibraryStats,
  OcrSetting,
} from "./protocol";

export type DocumentTaskKind = "sync" | "add" | "reprocess";
export type DocumentTaskState = "running" | "completed" | "cancelled" | "failed";

export interface DocumentTaskFailure {
  path: string;
  error: string;
}

/** App-lifetime ingest state. Keeping this outside `DocsView` means switching screens does not lose a running task. */
export interface DocumentTask {
  id: string;
  scope: string;
  kind: DocumentTaskKind;
  state: DocumentTaskState;
  progress: IngestProgress;
  startedAt: number;
  finishedAt: number | null;
  stored: number;
  skipped: number;
  failures: DocumentTaskFailure[];
  warning: string | null;
  error: string | null;
  documents: DocumentView[] | null;
}

const [documentTasks, setDocumentTasks] = createSignal<Record<string, DocumentTask>>({});
const runningTasks = new Map<string, Promise<DocumentView[]>>();
let taskSequence = 0;

export { documentTasks };

function updateDocumentTask(
  scope: string,
  id: string,
  update: (task: DocumentTask) => DocumentTask,
): DocumentTask | undefined {
  let updated: DocumentTask | undefined;
  setDocumentTasks((all) => {
    const current = all[scope];
    if (current === undefined || current.id !== id) return all;
    updated = update(current);
    return { ...all, [scope]: updated };
  });
  return updated;
}

const documentNotificationId = (id: string) => `document:${id}`;

function documentTaskTitle(kind: DocumentTaskKind): string {
  if (kind === "sync") return t(S.docs.ingest.kindSync);
  if (kind === "add") return t(S.docs.ingest.kindAdd);
  return t(S.docs.ingest.kindReprocess);
}

function documentTaskCount(task: DocumentTask): string {
  const frame = task.progress;
  if (frame.total <= 0) return "";
  if (frame.stage === "embedding") {
    return t(S.docs.ingest.chunks, { done: frame.done, total: frame.total });
  }
  if (frame.stage === "ocr") {
    return t(S.docs.ingest.pages, { done: frame.done, total: frame.total });
  }
  return t(S.docs.ingest.files, { done: frame.done, total: frame.total });
}

function syncDocumentTaskNotification(task: DocumentTask, announce: boolean): void {
  const running = task.state === "running";
  const summary = t(S.docs.ingest.summary, {
    stored: task.stored,
    skipped: task.skipped,
    failed: task.failures.length,
  });
  const count = documentTaskCount(task);
  const status =
    task.state === "failed"
      ? S.docs.ingest.statusFailed
      : task.state === "cancelled"
        ? S.docs.ingest.statusCancelled
        : S.docs.ingest.statusCompleted;
  const message = running
    ? `${stageLabel(task.progress.stage)}${count === "" ? "" : ` · ${count}`}`
    : `${t(status)} · ${summary}`;
  const detail =
    task.error ??
    task.warning ??
    (task.failures[0] === undefined
      ? task.progress.path
      : `${task.failures[0].path}: ${task.failures[0].error}`);
  const tone = running
    ? "progress"
    : task.state === "failed"
      ? "error"
      : task.state === "cancelled"
        ? "info"
      : task.warning !== null || task.failures.length > 0
        ? "warning"
        : "success";

  upsertAppNotification(
    {
      id: documentNotificationId(task.id),
      tone,
      title: documentTaskTitle(task.kind),
      message,
      detail: detail || undefined,
      progress: running
        ? {
            done: task.progress.done,
            total: task.progress.total,
            label: `${stageLabel(task.progress.stage)}${count === "" ? "" : `, ${count}`}`,
          }
        : undefined,
      dismissible: !running,
    },
    announce,
  );
}

/** Run at most one document mutation per project while exposing its lifecycle to any mounted screen. */
export function runDocumentTask(
  scope: string,
  kind: DocumentTaskKind,
  initial: IngestProgress,
  execute: (onProgress: (progress: IngestProgress) => void) => Promise<DocumentView[]>,
): Promise<DocumentView[]> {
  const active = runningTasks.get(scope);
  if (active !== undefined) return active;

  const id = `${Date.now()}-${++taskSequence}`;
  const task: DocumentTask = {
    id,
    scope,
    kind,
    state: "running",
    progress: initial,
    startedAt: Date.now(),
    finishedAt: null,
    stored: 0,
    skipped: 0,
    failures: [],
    warning: null,
    error: null,
    documents: null,
  };
  setDocumentTasks((all) => ({ ...all, [scope]: task }));
  syncDocumentTaskNotification(task, true);

  const note = (progress: IngestProgress) => {
    const updated = updateDocumentTask(scope, id, (current) => {
      if (current.state !== "running") return current;
      const fatal = progress.stage === "failed" && progress.error !== null && progress.total === 0;
      const failure =
        progress.stage === "failed" && progress.error !== null && !fatal
          ? [{ path: progress.path, error: progress.error }]
          : [];
      return {
        ...current,
        state: progress.stage === "cancelled" ? "cancelled" : current.state,
        finishedAt: progress.stage === "cancelled" ? Date.now() : current.finishedAt,
        progress,
        stored: current.stored + (progress.stage === "stored" ? 1 : 0),
        skipped: current.skipped + (progress.stage === "skipped" ? 1 : 0),
        failures: [...current.failures, ...failure],
        warning:
          progress.stage === "embedding" && progress.error !== null
            ? progress.error
            : current.warning,
        error: fatal ? progress.error : current.error,
      };
    });
    if (updated !== undefined) syncDocumentTaskNotification(updated, false);
  };

  const promise = execute(note)
    .then((documents) => {
      const updated = updateDocumentTask(scope, id, (current) => ({
        ...current,
        state:
          current.state === "cancelled"
            ? "cancelled"
            : current.error === null
              ? "completed"
              : "failed",
        finishedAt: Date.now(),
        documents,
      }));
      if (updated !== undefined) syncDocumentTaskNotification(updated, true);
      return documents;
    })
    .catch((error: unknown) => {
      const updated = updateDocumentTask(scope, id, (current) => ({
        ...current,
        state: "failed",
        finishedAt: Date.now(),
        error: String(error),
      }));
      if (updated !== undefined) syncDocumentTaskNotification(updated, true);
      throw error;
    })
    .finally(() => {
      if (runningTasks.get(scope) === promise) runningTasks.delete(scope);
    });
  runningTasks.set(scope, promise);
  return promise;
}

export function dismissDocumentTask(scope: string) {
  const current = documentTasks()[scope];
  if (current === undefined || current.state === "running") return;
  dismissAppNotification(documentNotificationId(current.id));
  setDocumentTasks((all) => {
    const next = { ...all };
    delete next[scope];
    return next;
  });
}

/** Ask the Rust library to stop its active extraction/OCR/embedding stream. */
export function stopDocumentIndexing(): Promise<boolean> {
  if (!inTauri()) return Promise.resolve(false);
  return invoke<boolean>("stop_document_indexing");
}

/** Stop the persistent task for one project. Browser/demo mode has no Rust stream, so finish it locally. */
export async function stopDocumentTask(scope: string): Promise<boolean> {
  const current = documentTasks()[scope];
  if (current === undefined || current.state !== "running") return false;
  if (inTauri()) return stopDocumentIndexing();

  const updated = updateDocumentTask(scope, current.id, (task) => ({
    ...task,
    state: "cancelled",
    finishedAt: Date.now(),
    progress: { ...task.progress, stage: "cancelled", finished: true },
  }));
  if (updated !== undefined) syncDocumentTaskNotification(updated, true);
  return true;
}

export async function getOcrSetting(): Promise<OcrSetting> {
  if (!inTauri()) return { enabled: true, visionModel: null };
  try {
    return await invoke<OcrSetting>("ocr_setting");
  } catch (err) {
    console.error("failed to read OCR setting", err);
    return { enabled: true, visionModel: null };
  }
}

export function setOcrEnabled(enabled: boolean): Promise<OcrSetting> {
  return invoke<OcrSetting>("set_ocr_enabled", { enabled });
}

/** The document library commands. Screen-load calls swallow errors and return honest empty values; anything
 * behind a click throws, because silence after a click is indistinguishable from slowness. */

export async function listDocuments(): Promise<DocumentView[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<DocumentView[]>("list_documents");
  } catch (err) {
    console.error("failed to list documents", err);
    return [];
  }
}

/** Library health; when unreachable it reports `semanticReady: false` with a reason, not stats that look empty. */
export async function libraryStats(): Promise<LibraryStats> {
  const unknown: LibraryStats = {
    documents: 0,
    chunks: 0,
    embeddedChunks: 0,
    embedder: null,
    semanticReady: false,
    reason: t(S.docs.error.stats),
    root: "",
    filesSeen: 0,
    filesSkipped: 0,
    unreadable: 0,
    excluded: 0,
    // `null`, not `0`: we do not know whether a scan ever ran, and `0` would render as 1/1/1970.
    scannedAt: null,
    scanning: null,
  };
  if (!inTauri()) return unknown;
  try {
    return await invoke<LibraryStats>("library_stats");
  } catch (err) {
    console.error("failed to read library stats", err);
    return unknown;
  }
}

/** Ingest documents, progress over a `Channel`; returns the library after ingest, and throws only if the whole batch fails. */
export function addDocuments(
  paths: string[],
  onProgress: (p: IngestProgress) => void,
): Promise<DocumentView[]> {
  const channel = new Channel<IngestProgress>();
  channel.onmessage = onProgress;
  return invoke<DocumentView[]>("add_documents", { paths, onProgress: channel });
}

/** Remove a document from the library, along with every chunk cut from it. */
/** Rescan the project directory on screen open, not on a button: the core skips unchanged files, so it is near free. */
export function syncLibrary(onProgress: (p: IngestProgress) => void): Promise<DocumentView[]> {
  if (!inTauri()) return Promise.resolve([]);
  const channel = new Channel<IngestProgress>();
  channel.onmessage = onProgress;
  return invoke<DocumentView[]>("sync_library", { onProgress: channel });
}

/** Reprocess the whole library after a click; throws rather than swallowing, unlike `syncLibrary`. */
export function reprocessLibrary(
  onProgress: (p: IngestProgress) => void,
): Promise<DocumentView[]> {
  const channel = new Channel<IngestProgress>();
  channel.onmessage = onProgress;
  return invoke<DocumentView[]>("reprocess_library", { onProgress: channel });
}

export function removeDocument(id: string): Promise<void> {
  return invoke("remove_document", { id });
}

/** Probe search: `limit` has a default because the probe answers "can the library find this", not "show me everything". */
export function searchDocuments(query: string, limit = 8): Promise<DocumentHit[]> {
  return invoke<DocumentHit[]>("search_documents", { query, limit });
}

/** OS file dialog; an empty array means the user cancelled. Unfiltered, since the readable-format list lives in Rust. */
export async function pickDocuments(): Promise<string[]> {
  if (!inTauri()) return [];
  const picked = await open({ directory: false, multiple: true });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/* --- Display helpers, so the document table does not invent its own naming --- */

export function formatLabel(format: DocumentFormat): string {
  return t(S.docs.format[format]);
}

/** Human file size, base 1024, one decimal from MB up, where the fraction is a real difference in load time. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/** Three embedding states, kept out of JSX because both the table and the stats strip read them. */
export type EmbedState = "embedded" | "queued" | "failed";

/** `embedded === false` with `error === null` means queued, not failed; conflating the two makes users re-add good files. */
export function embedState(doc: DocumentView): EmbedState {
  if (doc.error !== null) return "failed";
  return doc.embedded ? "embedded" : "queued";
}

/** Translated ingest stage name; `stage` is a core key, and an unknown one is returned verbatim rather than blank. */
export function stageLabel(stage: string): string {
  const table: Record<string, Msg> = S.docs.stage;
  const msg = table[stage];
  return msg === undefined ? stage : t(msg);
}
