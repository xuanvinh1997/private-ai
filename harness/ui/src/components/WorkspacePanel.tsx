import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import type { ChangedFile } from "../lib/changes";
import { S, t } from "../lib/i18n";
import type { Project } from "../lib/protocol";
import {
  setWorkspacePanelWidth,
  workspacePanelWidth,
  WORKSPACE_PANEL_WIDTH,
} from "../lib/prefs";
import { ChangesPanelContent } from "./ChangesPanel";
import Icon, { type IconName } from "./Icon";
import { IconButton } from "./primitives";
import ResizeHandle from "./ResizeHandle";
import { ProjectFilesContent } from "./projects/ProjectPanel";

export type WorkspacePanelTab = "changes" | "files";

const TABS: readonly WorkspacePanelTab[] = ["changes", "files"];

/** Workspace inspector: one right column, two views of the same project. Tabs keep the position, width and close
 * button; below 1048px it overlays instead of squeezing the reading column, and both tabs stay mounted. */
export default function WorkspacePanel(props: {
  tab: WorkspacePanelTab;
  files: ChangedFile[];
  project: Project;
  onTab: (tab: WorkspacePanelTab) => void;
  onReveal: (nodeId: string) => void;
  onPickFile: (path: string) => void;
  onOpenScreen: () => void;
  onClose: () => void;
  focusOnMount?: boolean;
}) {
  let panel: HTMLElement | undefined;
  let tabList: HTMLDivElement | undefined;
  const uid = createUniqueId();
  const tabId = (tab: WorkspacePanelTab) => `${uid}-${tab}-tab`;
  const panelId = (tab: WorkspacePanelTab) => `${uid}-${tab}-panel`;
  const label = (tab: WorkspacePanelTab) =>
    tab === "changes" ? t(S.chat.changes.title) : t(S.projects.panelTitle);
  const icon = (tab: WorkspacePanelTab): IconName => (tab === "changes" ? "diff" : "folder");
  // Do not read the tree just because the inspector is open on the diff tab; after the first visit, keep it mounted.
  const [filesVisited, setFilesVisited] = createSignal(props.tab === "files");
  createEffect(() => {
    if (props.tab === "files") setFilesVisited(true);
  });
  onMount(() => {
    if (props.focusOnMount) {
      queueMicrotask(() =>
        tabList
          ?.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')
          ?.focus(),
      );
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || panel?.closest("[inert]")) return;
      event.preventDefault();
      props.onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    onCleanup(() => window.removeEventListener("keydown", closeOnEscape));
  });

  return (
    <aside
      ref={panel}
      aria-label={t(S.chat.inspector.label)}
      style={{ width: `${workspacePanelWidth()}px` }}
      class="absolute inset-y-0 right-0 z-[var(--z-floating)] flex max-w-[calc(100vw-48px)] shrink-0 flex-col border-l border-line bg-surface shadow-pop min-[1048px]:static min-[1048px]:max-w-[40vw] min-[1048px]:shadow-none"
    >
      <ResizeHandle
        edge="left"
        label={t(S.chat.inspector.resize)}
        value={workspacePanelWidth()}
        min={WORKSPACE_PANEL_WIDTH.min}
        max={WORKSPACE_PANEL_WIDTH.max}
        defaultValue={WORKSPACE_PANEL_WIDTH.default}
        onChange={setWorkspacePanelWidth}
      />
      <header class="flex h-(--header-h) shrink-0 items-center border-b border-line px-sm">
        <div
          ref={tabList}
          role="tablist"
          aria-label={t(S.chat.inspector.tabs)}
          class="flex h-full min-w-0 flex-1 items-stretch gap-3xs"
          onKeyDown={(event) => {
            if (![
              "ArrowLeft",
              "ArrowRight",
              "ArrowUp",
              "ArrowDown",
              "Home",
              "End",
            ].includes(event.key)) {
              return;
            }
            event.preventDefault();
            const current = TABS.indexOf(props.tab);
            const next =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? TABS.length - 1
                  : (current +
                      (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) +
                      TABS.length) %
                    TABS.length;
            const target = TABS[next];
            if (target === undefined) return;
            props.onTab(target);
            event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
          }}
        >
          <For each={TABS}>
            {(tab) => (
              <button
                type="button"
                id={tabId(tab)}
                role="tab"
                aria-selected={props.tab === tab}
                aria-controls={panelId(tab)}
                tabIndex={props.tab === tab ? 0 : -1}
                onClick={() => props.onTab(tab)}
                class="flex min-w-0 items-center gap-2xs border-b-2 px-xs text-xs font-medium transition-colors duration-[var(--dur-fast)]"
                classList={{
                  "border-accent text-accent-ink": props.tab === tab,
                  "border-transparent text-muted hover:text-ink": props.tab !== tab,
                }}
              >
                <Icon name={icon(tab)} size={13} />
                <span class="min-w-0 truncate">{label(tab)}</span>
                {tab === "changes" && props.files.length > 0 ? (
                  <span class="grid min-w-5 place-items-center rounded-pill bg-accent-soft px-3xs text-2xs text-accent-ink tabular-nums">
                    {props.files.length}
                  </span>
                ) : null}
              </button>
            )}
          </For>
        </div>

        <IconButton
          icon="x"
          label={t(S.chat.inspector.close)}
          size="sm"
          onClick={props.onClose}
        />
      </header>

      <section
        id={panelId("changes")}
        role="tabpanel"
        aria-labelledby={tabId("changes")}
        class="min-h-0 flex-1"
        classList={{ flex: props.tab === "changes", hidden: props.tab !== "changes" }}
      >
        <ChangesPanelContent files={props.files} onReveal={props.onReveal} />
      </section>

      <section
        id={panelId("files")}
        role="tabpanel"
        aria-labelledby={tabId("files")}
        class="min-h-0 flex-1"
        classList={{ flex: props.tab === "files", hidden: props.tab !== "files" }}
      >
        <Show when={filesVisited()}>
          <ProjectFilesContent
            project={props.project}
            onPickFile={props.onPickFile}
            onOpenScreen={props.onOpenScreen}
          />
        </Show>
      </section>
    </aside>
  );
}
