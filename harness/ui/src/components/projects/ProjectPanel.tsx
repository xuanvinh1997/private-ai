import { createResource, createSignal, For, Show, Suspense } from "solid-js";
import { addDocuments, stageLabel } from "../../lib/docs";
import { S, t } from "../../lib/i18n";
import { listDir, originHost } from "../../lib/projects";
import { relativeTime } from "../../lib/sessions";
import type { DirEntry, Project } from "../../lib/protocol";
import Icon from "../Icon";
import { IconButton } from "../primitives";
import { Button } from "./DialogShell";
import UploadFilesDialog from "./UploadFilesDialog";

/** The open project's file tree in the right column; it reads one level per expand, so `.git` and `node_modules` cost nothing until opened, and clicking a file drops an `@path` into the composer. */
export function ProjectFilesContent(props: {
  project: Project;
  onOpenFolder: () => void;
  /** Put a file into the composer; takes an absolute path, the caller shortens it. */
  onPickFile: (path: string) => void;
  /** Open the project's own screen: Changes for code, Library for documents. */
  onOpenScreen: () => void;
}) {
  const docs = () => props.project.kind === "docs";
  const [uploadOpen, setUploadOpen] = createSignal(false);
  const [treeRevision, setTreeRevision] = createSignal(0);

  return (
    <div class="flex min-h-0 flex-1 flex-col">
      {/* Name and kind, enough to say whose tree this is; the full path lives in `title`. */}
      <div
        class="flex shrink-0 items-center gap-sm border-b border-line px-md py-sm"
        title={props.project.path}
      >
        <span class="grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
          <Icon name={docs() ? "library" : "code"} size={14} />
        </span>
        <div class="flex min-w-0 flex-1 flex-col gap-3xs">
          <span class="truncate text-xs font-medium text-ink">{props.project.name}</span>
          <span class="truncate text-2xs text-faint">
            {t(S.projects.panelMeta, {
              kind: docs() ? t(S.projects.kindDocs) : t(S.projects.kindCodeLong),
              when: relativeTime(props.project.lastOpenedAt),
            })}
            <Show when={props.project.origin}>
              {(origin) => <> · {originHost(origin())}</>}
            </Show>
          </span>
        </div>
        {/* These two actions belong to the tree, so they sit here and not in the inspector tablist. */}
        <IconButton
          icon={docs() ? "library" : "diff"}
          label={docs() ? t(S.projects.openLibrary) : t(S.projects.openChanges)}
          size="sm"
          onClick={props.onOpenScreen}
        />
        <IconButton
          icon="external"
          label={t(S.projects.openInFileManager)}
          size="sm"
          onClick={props.onOpenFolder}
        />
      </div>

      <div class="flex shrink-0 items-center border-b border-line px-sm py-sm">
        <Button variant="outline" icon="upload" onClick={() => setUploadOpen(true)}>
          {t(S.projects.uploadFiles)}
        </Button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-2xs">
        {/* `keyed` by path: a new project is a new tree, and stale expansion points at gone folders. */}
        <Show when={props.project.path} keyed>
          {(root) => (
            <Branch
              path={root}
              depth={0}
              revision={treeRevision()}
              canReindex={docs()}
              onPickFile={props.onPickFile}
            />
          )}
        </Show>
      </div>

      <Show when={uploadOpen()}>
        <UploadFilesDialog
          project={props.project}
          onClose={() => setUploadOpen(false)}
          onImported={() => setTreeRevision((revision) => revision + 1)}
        />
      </Show>
    </div>
  );
}

/** One level of the tree; each branch fetches its own contents, so only that branch shows waiting. */
function Branch(props: {
  path: string;
  depth: number;
  revision: number;
  canReindex: boolean;
  onPickFile: (path: string) => void;
}) {
  const [entries] = createResource(
    () => ({ path: props.path, revision: props.revision }),
    ({ path }) => listDir(path),
  );

  return (
    <Suspense fallback={<Line depth={props.depth}>{t(S.projects.reading)}</Line>}>
      <Show
        when={(entries() ?? []).length > 0}
        fallback={<Line depth={props.depth}>{t(S.projects.emptyDir)}</Line>}
      >
        <ul class="m-0 flex list-none flex-col p-0">
          <For each={entries()}>
            {(entry) => (
              <Node
                entry={entry}
                depth={props.depth}
                revision={props.revision}
                canReindex={props.canReindex}
                onPickFile={props.onPickFile}
              />
            )}
          </For>
        </ul>
      </Show>
    </Suspense>
  );
}

/** One row: a folder expands, a file goes to the composer. */
function Node(props: {
  entry: DirEntry;
  depth: number;
  revision: number;
  canReindex: boolean;
  onPickFile: (path: string) => void;
}) {
  const [open, setOpen] = createSignal(false);
  const [reindexState, setReindexState] = createSignal<
    "idle" | "busy" | "success" | "error"
  >("idle");
  const [reindexDetail, setReindexDetail] = createSignal<string | null>(null);

  const reindexLabel = () => {
    const name = props.entry.name;
    if (reindexState() === "busy") return t(S.projects.reindexingFile, { name });
    if (reindexState() === "success") return t(S.projects.reindexedFile, { name });
    if (reindexState() === "error") return t(S.projects.retryReindexFile, { name });
    return t(S.projects.reindexFile, { name });
  };

  const reindexIcon = () => {
    if (reindexState() === "success") return "check" as const;
    if (reindexState() === "error") return "retry" as const;
    return "refresh" as const;
  };

  const reindex = async () => {
    if (props.entry.isDir || reindexState() === "busy") return;
    let failure: string | null = null;
    setReindexState("busy");
    setReindexDetail(t(S.projects.reindexPreparing));
    try {
      await addDocuments([props.entry.path], (frame) => {
        setReindexDetail(stageLabel(frame.stage));
        if (frame.error !== null) failure = frame.error;
      });
      if (failure === null) {
        setReindexState("success");
        setReindexDetail(null);
      } else {
        setReindexState("error");
        setReindexDetail(
          t(S.projects.reindexError, { name: props.entry.name, err: failure }),
        );
      }
    } catch (err) {
      setReindexState("error");
      setReindexDetail(
        t(S.projects.reindexError, { name: props.entry.name, err: String(err) }),
      );
    }
  };

  return (
    <li>
      <div
        // Indent by depth plus room for the chevron, which files keep so names stay aligned.
        style={{ "padding-left": `${props.depth * 12 + 4}px` }}
        class="flex w-full items-center rounded-panel pr-2xs transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] focus-within:bg-[var(--overlay-hover)]"
      >
        <button
          type="button"
          onClick={() =>
            props.entry.isDir ? setOpen((v) => !v) : props.onPickFile(props.entry.path)
          }
          title={
            props.entry.isDir
              ? props.entry.name
              : t(S.projects.pickFileTip, { name: props.entry.name })
          }
          aria-expanded={props.entry.isDir ? open() : undefined}
          class="flex min-w-0 flex-1 items-center gap-2xs py-3xs text-left"
        >
          <span class="w-3 shrink-0 text-faint">
            <Show when={props.entry.isDir}>
              <Icon
                name="chevron-right"
                size={11}
                class={`transition-transform duration-[var(--dur-fast)] ${open() ? "rotate-90" : ""}`}
              />
            </Show>
          </span>
          <span class="shrink-0 text-muted">
            <Icon name={props.entry.isDir ? "folder" : "document"} size={13} />
          </span>
          <span class="min-w-0 flex-1 truncate text-2xs text-text">{props.entry.name}</span>
        </button>

        <Show when={props.canReindex && !props.entry.isDir}>
          <IconButton
            icon={reindexIcon()}
            label={reindexLabel()}
            size="sm"
            tip="left"
            busy={reindexState() === "busy"}
            onClick={() => void reindex()}
          />
        </Show>
      </div>

      <Show when={reindexDetail()}>
        {(detail) => (
          <p
            class="m-0 py-3xs pr-sm text-2xs break-words"
            classList={{
              "text-muted": reindexState() === "busy",
              "text-danger": reindexState() === "error",
            }}
            style={{ "padding-left": `${props.depth * 12 + 24}px` }}
            role={reindexState() === "error" ? "alert" : "status"}
            aria-live="polite"
          >
            {reindexState() === "busy" ? `${detail()}: ${props.entry.name}` : detail()}
          </p>
        )}
      </Show>

      {/* Child branches are built on first expand and torn down on collapse, so nothing goes stale. */}
      <Show when={props.entry.isDir && open()}>
        <Branch
          path={props.entry.path}
          depth={props.depth + 1}
          revision={props.revision}
          canReindex={props.canReindex}
          onPickFile={props.onPickFile}
        />
      </Show>
    </li>
  );
}

/** A branch's status line, reading or empty, indented to match its rows. */
function Line(props: { depth: number; children: string }) {
  return (
    <p
      class="m-0 py-3xs text-2xs text-faint"
      style={{ "padding-left": `${props.depth * 12 + 24}px` }}
    >
      {props.children}
    </p>
  );
}
