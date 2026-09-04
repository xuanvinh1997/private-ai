import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import { S, t, type Msg } from "./i18n";
import type {
  DocumentFormat,
  DocumentHit,
  DocumentView,
  IngestProgress,
  LibraryStats,
} from "./protocol";

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
