import { Key } from "@solid-primitives/keyed";
import { createMemo, createSignal, For, Show } from "solid-js";
import { useDragDrop } from "../../hooks/useDragDrop";
import { S, t, type Msg } from "../../lib/i18n";
import { originHost } from "../../lib/projects";
import type { Project, ProjectKind } from "../../lib/protocol";
import { relativeTime } from "../../lib/sessions";
import Icon, { type IconName } from "../Icon";
import { Chip, IconButton } from "../primitives";
import { InfoDot } from "../settings/FormKit";
import CloneDialog from "./CloneDialog";
import ConfirmDialog from "./ConfirmDialog";
import { Button } from "./DialogShell";
import NewProjectDialog from "./NewProjectDialog";

type Filter = "all" | ProjectKind;

/** The projects screen as a full page, not a dropdown: it has room for search, filters and the three ways in. Its wording says "remove from the list" rather than "delete", and the open project shows up front that it cannot be removed. */
export default function ProjectsView(props: {
  projects: Project[];
  /** The core is remounting the plugin branch, so the page locks until it finishes. */
  switching?: boolean;
  /** Error from the last open or removal, held by the caller. */
  error?: string | null;
  onOpen: (project: Project) => void;
  /** Open a folder that is not in the list yet; drag and drop is its only entrance. */
  onOpenPath: (path: string) => void;
  onForget: (project: Project) => void;
  /** Delete the project's conversations and library as well; the folder on disk stays. */
  onDelete: (project: Project) => void;
  /** A delete in flight: dropping a library starts the document service, so it is not instant. */
  deleting?: boolean;
  /** The core finished creating or cloning; the caller reloads and switches to it. */
  onCreated: (project: Project) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [filter, setFilter] = createSignal<Filter>("all");
  const [newKind, setNewKind] = createSignal<ProjectKind | null>(null);
  const [cloning, setCloning] = createSignal(false);
  const [forgetting, setForgetting] = createSignal<Project | null>(null);

  /** A drop opens a folder as a project, and this screen owns that gesture; it defers while any dialog is open, since the drop belongs to the dialog then. */
  useDragDrop((paths) => {
    if (props.switching === true) return;
    if (newKind() !== null || cloning() || forgetting() !== null) return;
    const first = paths[0];
    if (first !== undefined) props.onOpenPath(first);
  });

  // Newest first; the open project is not pinned, as it is already marked and pinning reorders.
  const visible = createMemo(() => {
    const needle = query().trim().toLowerCase();
    const kind = filter();
    return props.projects
      .filter((project) => kind === "all" || project.kind === kind)
      .filter(
        (project) =>
          needle === "" ||
          project.name.toLowerCase().includes(needle) ||
          project.path.toLowerCase().includes(needle),
      )
      .sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  });

  const counts = createMemo(() => ({
    all: props.projects.length,
    code: props.projects.filter((p) => p.kind === "code").length,
    docs: props.projects.filter((p) => p.kind === "docs").length,
  }));

  return (
    <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) py-(--page-pad-y)">
      <div class="mx-auto flex max-w-[880px] flex-col gap-2xl">
        <section class="flex flex-col gap-md">
          <div class="flex items-start gap-sm">
            <span class="mt-3xs grid size-7 shrink-0 place-items-center rounded-panel bg-accent-soft text-accent-ink">
              <Icon name="folder" size={15} />
            </span>
            <div class="flex min-w-0 flex-col gap-3xs">
              <h2 class="m-0 flex items-center gap-2xs text-md font-medium text-ink">
                {t(S.projects.title)}
                <InfoDot text={t(S.projects.scopeHint)} />
              </h2>
              <p class="m-0 text-xs text-muted">{t(S.projects.subtitle)}</p>
            </div>
          </div>

          <div class="grid gap-sm sm:grid-cols-3">
            <For each={ENTRANCES}>
              {(entrance) => (
                <button
                  type="button"
                  disabled={props.switching}
                  onClick={() => {
                    if (entrance.id === "clone") setCloning(true);
                    else setNewKind(entrance.id);
                  }}
                  class="flex flex-col gap-2xs rounded-card border border-line bg-surface px-(--card-pad-x) py-(--card-pad-y) text-left transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:border-accent enabled:hover:bg-accent-soft"
                >
                  <span class="flex items-center gap-2xs text-sm font-medium text-ink">
                    <Icon name={entrance.icon} size={15} />
                    {t(entrance.label)}
                  </span>
                  <span class="text-2xs text-muted">{t(entrance.hint)}</span>
                </button>
              )}
            </For>
          </div>
        </section>

        <Show when={props.error}>
          {(message) => (
            <p class="m-0 rounded-panel bg-danger-soft px-sm py-2xs text-xs break-words text-danger" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <section class="flex flex-col gap-md">
          <div class="flex flex-wrap items-center gap-sm">
            <label class="flex min-w-[220px] flex-1 items-center gap-2xs rounded-btn border border-line-strong bg-surface px-sm transition-colors duration-[var(--dur-fast)] focus-within:border-accent">
              <span class="shrink-0 text-faint">
                <Icon name="search" size={14} />
              </span>
              <input
                type="search"
                value={query()}
                spellcheck={false}
                placeholder={t(S.projects.searchPlaceholder)}
                aria-label={t(S.projects.searchLabel)}
                onInput={(event) => setQuery(event.currentTarget.value)}
                class="h-(--control-h) min-w-0 flex-1 bg-transparent text-xs text-text outline-none placeholder:text-faint"
              />
            </label>

            <div
              role="radiogroup"
              aria-label={t(S.projects.filterLabel)}
              class="flex gap-2xs"
              onKeyDown={(event) => {
                if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
                event.preventDefault();
                const buttons = [
                  ...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
                ];
                const current = Math.max(
                  0,
                  buttons.indexOf(document.activeElement as HTMLButtonElement),
                );
                const delta = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
                const next = (current + delta + buttons.length) % buttons.length;
                const option = FILTERS[next];
                buttons[next]?.focus();
                if (option !== undefined) setFilter(option.id);
              }}
            >
              <For each={FILTERS}>
                {(option) => (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={filter() === option.id}
                    tabIndex={filter() === option.id ? 0 : -1}
                    onClick={() => setFilter(option.id)}
                    class="flex h-(--control-h) items-center gap-2xs rounded-pill border px-md text-xs font-medium transition-colors duration-[var(--dur-fast)]"
                    classList={{
                      "border-line text-muted hover:bg-[var(--overlay-hover)] hover:text-ink":
                        filter() !== option.id,
                      "border-accent bg-accent-soft text-accent-ink": filter() === option.id,
                    }}
                  >
                    {t(option.label)}
                    <span class="tabular-nums">{counts()[option.id]}</span>
                  </button>
                )}
              </For>
            </div>
          </div>

          <Show
            when={props.projects.length > 0}
            fallback={
              <div class="flex flex-col items-center gap-md rounded-card border border-dashed border-line px-(--card-pad-x) py-4xl text-center">
                <span class="grid size-12 place-items-center rounded-panel bg-accent-soft text-accent-ink">
                  <Icon name="folder-open" size={24} />
                </span>
                <div class="flex flex-col gap-2xs">
                  <p class="m-0 text-sm font-medium text-ink">{t(S.projects.emptyTitle)}</p>
                  <p class="m-0 max-w-[44ch] text-xs text-muted">{t(S.projects.emptyHint)}</p>
                </div>
                <div class="flex flex-wrap justify-center gap-sm">
                  <Button variant="outline" icon="folder-open" onClick={() => setNewKind("code")}>
                    {t(S.projects.newCode)}
                  </Button>
                  <Button variant="outline" icon="library" onClick={() => setNewKind("docs")}>
                    {t(S.projects.newDocs)}
                  </Button>
                </div>
              </div>
            }
          >
            <Show
              when={visible().length > 0}
              fallback={
                <p class="m-0 rounded-card border border-dashed border-line px-(--card-pad-x) py-2xl text-center text-xs text-muted">
                  {t(S.projects.noMatch)}
                </p>
              }
            >
              <ul class="m-0 flex list-none flex-col gap-sm p-0">
                {/* Keyed by id: the list reorders on every open, and index keying would drop focus. */}
                <Key each={visible()} by={(project) => project.id}>
                  {(keyed) => (
                    <Row
                      project={keyed()}
                      disabled={props.switching === true}
                      onOpen={() => props.onOpen(keyed())}
                      onForget={() => setForgetting(keyed())}
                    />
                  )}
                </Key>
              </ul>
            </Show>
          </Show>
        </section>
      </div>

      <Show when={newKind()}>
        {(kind) => (
          <NewProjectDialog
            kind={kind()}
            onClose={() => setNewKind(null)}
            onCreated={(project) => {
              setNewKind(null);
              props.onCreated(project);
            }}
          />
        )}
      </Show>

      <Show when={cloning()}>
        <CloneDialog
          onClose={() => setCloning(false)}
          onCreated={(project) => {
            setCloning(false);
            props.onCreated(project);
          }}
        />
      </Show>

      <Show when={forgetting()}>
        {(project) => (
          <ConfirmDialog
            icon="trash"
            title={t(S.projects.forgetTitle, { name: project().name })}
            body={t(S.projects.forgetOrDeleteBody)}
            more={t(S.projects.forgetMore)}
            detail={project().path}
            confirmLabel={t(S.projects.forgetConfirm)}
            busy={props.deleting === true}
            escalate={{
              label: t(S.projects.deleteConfirm),
              onClick: () => {
                const target = project();
                setForgetting(null);
                props.onDelete(target);
              },
            }}
            onClose={() => setForgetting(null)}
            onConfirm={() => {
              const target = project();
              setForgetting(null);
              props.onForget(target);
            }}
          />
        )}
      </Show>
    </div>
  );
}

/** One project as a row, not a card: a long path needs the full width to stay readable. */
function Row(props: {
  project: Project;
  disabled: boolean;
  onOpen: () => void;
  onForget: () => void;
}) {
  const current = () => props.project.isCurrent;
  return (
    <li
      aria-current={current() ? "true" : undefined}
      class="flex items-center gap-md rounded-card border bg-surface px-(--card-pad-x) py-(--card-pad-y) transition-colors duration-[var(--dur-fast)]"
      classList={{
        "border-line": !current(),
        "border-accent bg-accent-soft": current(),
      }}
    >
      <span
        class="grid size-8 shrink-0 place-items-center rounded-panel"
        classList={{
          "bg-accent text-on-accent": current(),
          "bg-[var(--overlay-faint)] text-muted": !current(),
        }}
      >
        <Icon name={props.project.kind === "docs" ? "library" : "code"} size={15} />
      </span>

      <div class="flex min-w-0 flex-1 flex-col gap-3xs">
        <div class="flex flex-wrap items-center gap-2xs">
          <span class="min-w-0 truncate text-sm font-medium text-ink">{props.project.name}</span>
          <Chip>{props.project.kind === "docs" ? t(S.common.docs) : t(S.projects.kindCode)}</Chip>
          {/* The origin badge shows the host only; the full URL stays in `title`. */}
          <Show when={props.project.origin}>
            {(origin) => (
              <span title={origin()}>
                <Chip tone="accent">
                  <Icon name="git-branch" size={11} />
                  {originHost(origin())}
                </Chip>
              </span>
            )}
          </Show>
          <Show when={current()}>
            <Chip tone="accent">{t(S.projects.current)}</Chip>
          </Show>
        </div>
        {/* The path truncates at the start: two same-named projects differ at the end. */}
        <span class="min-w-0 truncate text-2xs text-faint" dir="rtl" title={props.project.path}>
          <bdi>{props.project.path}</bdi>
        </span>
      </div>

      <span class="hidden shrink-0 text-2xs whitespace-nowrap text-faint tabular-nums sm:inline">
        {relativeTime(props.project.lastOpenedAt)}
      </span>

      <div class="flex shrink-0 items-center gap-2xs">
        <Button
          variant="outline"
          disabled={props.disabled || current()}
          onClick={props.onOpen}
          label={t(current() ? S.projects.rowCurrentLabel : S.projects.rowOpenLabel, {
            name: props.project.name,
          })}
        >
          {current() ? t(S.projects.current) : t(S.common.open)}
        </Button>
        <IconButton
          icon="trash"
          size="sm"
          danger
          disabled={props.disabled || current()}
          onClick={props.onForget}
          label={t(current() ? S.projects.forgetBlockedLabel : S.projects.forgetLabel, {
            name: props.project.name,
          })}
          tip="left"
        />
      </div>
    </li>
  );
}

const FILTERS: { id: Filter; label: Msg }[] = [
  { id: "all", label: S.projects.filterAll },
  { id: "code", label: S.projects.kindCode },
  { id: "docs", label: S.common.docs },
];

/** Three entrances as three buttons, not one menu: a feature behind an extra click is invisible. */
const ENTRANCES: { id: ProjectKind | "clone"; label: Msg; icon: IconName; hint: Msg }[] = [
  {
    id: "code",
    label: S.projects.newCode,
    icon: "folder-open",
    hint: S.projects.newCodeHint,
  },
  {
    id: "clone",
    label: S.projects.cloneTitle,
    icon: "git-branch",
    hint: S.projects.cloneHint,
  },
  {
    id: "docs",
    label: S.projects.newDocs,
    icon: "library",
    hint: S.projects.newDocsHint,
  },
];
