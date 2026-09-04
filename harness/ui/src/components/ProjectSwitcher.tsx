import { Key } from "@solid-primitives/keyed";
import { createSignal, Show } from "solid-js";
import { S, t } from "../lib/i18n";
import type { Project, ProjectKind } from "../lib/protocol";
import Icon from "./Icon";
import Menu, { type MenuItem } from "./Menu";
import { InfoDot } from "./settings/FormKit";

/** How many projects show before the "see more" row. */
const HIEN_TRUOC = 5;

/** Project kind in words, for the accessible label an icon cannot carry. */
const kindLabel = (kind: ProjectKind) =>
  t(kind === "docs" ? S.projects.kindDocsInline : S.projects.kindCodeInline);

/** The "Projects" group in the sidebar: a first-class list rather than a menu behind a button, so it is visible
 * without asking. No action was dropped; per-row context menus carry kind, close and remove. */
export default function ProjectSwitcher(props: {
  projects: Project[];
  current: Project | null;
  /** The core is swapping plugin layers; everything here is locked while it does. */
  switching: boolean;
  /** Which row has its context menu open, by project id. */
  menuFor: string | null;
  onMenuChange: (id: string | null) => void;
  onPick: (id: string) => void;
  /** Open the projects screen, where create, clone and filtering live. */
  onSeeAll: () => void;
  onForget: (project: Project) => void;
  /** Close the open project; the list is unchanged, this is not `onForget`. */
  onClose: () => void;
  /** Change the open project's kind. */
  onSwapKind: (kind: ProjectKind) => void;
}) {
  const [expanded, setExpanded] = createSignal(false);

  // Newest first; the open project keeps its chronological place, since colour and icon already mark it.
  const ordered = () => [...props.projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  const visible = () => (expanded() ? ordered() : ordered().slice(0, HIEN_TRUOC));
  const hidden = () => ordered().length - visible().length;

  /** A row's context menu; the open project's three actions are the easiest to confuse, so each states its consequence. */
  const itemsFor = (project: Project): MenuItem[] => {
    const items: MenuItem[] = [];
    if (project.isCurrent) {
      // Kind is set once at registration, so without this row a mis-typed folder is a permanent dead end.
      items.push({
        id: "kind",
        label: t(project.kind === "code" ? S.projects.toDocs : S.projects.toCode),
        icon: project.kind === "code" ? "library" : "code",
        hint: t(project.kind === "code" ? S.projects.toDocsHint : S.projects.toCodeHint),
        onSelect: () => props.onSwapKind(project.kind === "code" ? "docs" : "code"),
      });
      items.push({
        id: "close",
        label: t(S.projects.closeProject),
        icon: "folder",
        hint: t(S.projects.closeProjectHint),
        onSelect: props.onClose,
      });
    } else {
      items.push({
        id: "open",
        label: t(S.projects.openThis),
        icon: "folder-open",
        onSelect: () => props.onPick(project.id),
      });
    }
    // Removing the open project would remove the ground you stand on; keep the row but disable it with a reason.
    items.push({
      id: "forget",
      label: t(S.projects.forgetConfirm),
      icon: "x",
      danger: !project.isCurrent,
      disabled: project.isCurrent,
      hint: t(project.isCurrent ? S.projects.forgetBlockedHint : S.projects.forgetSafeHint),
      onSelect: () => props.onForget(project),
    });
    return items;
  };

  return (
    <div class="flex flex-col gap-3xs">
      <Show
        when={ordered().length > 0}
        fallback={
          <p class="m-0 px-sm py-xs text-2xs text-faint">{t(S.projects.listEmpty)}</p>
        }
      >
        <ul class="m-0 flex list-none flex-col gap-3xs p-0">
          {/* Keyed by `id`: opening a project reorders the list, and keying by index would rebuild every row. */}
          <Key each={visible()} by="id">
            {(project) => (
              <li>
                <div
                  class="group/row relative flex items-center"
                  onContextMenu={(event) => {
                    event.preventDefault();
                    props.onMenuChange(project().id);
                  }}
                >
                  <button
                    type="button"
                    disabled={props.switching}
                    // Clicking a row opens its detail panel on the right, including the row that is already open.
                    onClick={() => props.onPick(project().id)}
                    aria-current={project().isCurrent ? "true" : undefined}
                    // The path no longer gets its own line; it moves into `title` and the accessible label.
                    title={project().path}
                    aria-label={t(
                      project().isCurrent ? S.projects.rowCurrentA11y : S.projects.rowOpenA11y,
                      {
                        name: project().name,
                        kind: kindLabel(project().kind),
                        path: project().path,
                      },
                    )}
                    class="flex min-w-0 flex-1 items-center gap-sm rounded-panel py-2xs pr-(--sp-2xl) pl-sm text-left text-sm transition-colors duration-[var(--dur-fast)] disabled:cursor-progress enabled:hover:bg-[var(--overlay-hover)] aria-[current]:bg-accent-soft aria-[current]:font-medium"
                  >
                    <span
                      class="shrink-0"
                      classList={{
                        "text-accent-ink": project().isCurrent,
                        "text-muted": !project().isCurrent,
                        "motion-safe:animate-pulse": props.switching && project().isCurrent,
                      }}
                    >
                      {/* The icon states the project *kind*, not open/closed, which three other cues already say.
                          Same icon pair as the Projects screen, so one object is not drawn two ways. */}
                      <Icon name={project().kind === "docs" ? "library" : "code"} size={15} />
                    </span>
                    <span
                      class="min-w-0 flex-1 truncate"
                      classList={{
                        "text-accent-ink": project().isCurrent,
                        "text-text": !project().isCurrent,
                      }}
                    >
                      {project().name}
                    </span>
                  </button>

                  <div
                    class="absolute right-3xs transition-opacity duration-[var(--dur-fast)] group-focus-within/row:opacity-100 group-hover/row:opacity-100"
                    classList={{
                      "opacity-0": props.menuFor !== project().id,
                      "opacity-100": props.menuFor === project().id,
                    }}
                  >
                    <Menu
                      label={t(S.projects.rowMenu, { name: project().name })}
                      open={props.menuFor === project().id}
                      onOpenChange={(open) => props.onMenuChange(open ? project().id : null)}
                      onRequestClose={() => props.onMenuChange(null)}
                      items={itemsFor(project())}
                    />
                  </div>
                </div>
              </li>
            )}
          </Key>
        </ul>
      </Show>

      {/* A long list is truncated, not scrolled: it would otherwise push "Recent" below the fold. */}
      <Show when={hidden() > 0 || expanded()}>
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded()}
          class="flex w-full items-center gap-2xs rounded-panel px-sm py-3xs text-left text-2xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink"
        >
          <Icon
            name="chevron-right"
            size={12}
            class={`transition-transform duration-[var(--dur-fast)] ${expanded() ? "rotate-90" : ""}`}
          />
          {expanded() ? t(S.projects.collapse) : t(S.projects.showMore, { n: hidden() })}
        </button>
      </Show>

      {/* One way out, to the projects screen rather than a second dialog: create, clone and filter live there. */}
      <button
        type="button"
        onClick={props.onSeeAll}
        disabled={props.switching}
        class="flex w-full items-center gap-sm rounded-panel px-sm py-2xs text-left text-xs text-muted transition-colors duration-[var(--dur-fast)] disabled:cursor-progress enabled:hover:bg-[var(--overlay-hover)] enabled:hover:text-ink"
      >
        <span class="shrink-0">
          <Icon name="more" size={15} />
        </span>
        {t(S.projects.seeAll)}
      </button>

      {/* "No project" is a valid state, not a pending load, so it says so even when the list above has rows. */}
      <Show when={props.current === null && !props.switching}>
        <p class="m-0 flex items-start gap-2xs rounded-panel bg-[var(--overlay-faint)] px-sm py-xs text-2xs leading-[1.5] text-muted">
          <span class="mt-3xs shrink-0 text-faint">
            <Icon name="chat" size={13} />
          </span>
          <span class="flex flex-wrap items-center gap-2xs">
            {t(S.projects.noProjectNote)}
            <InfoDot
              label={t(S.projects.noProjectLabel)}
              text={t(S.projects.noProjectMore)}
            />
          </span>
        </p>
      </Show>

      <Show when={props.switching}>
        <p class="m-0 px-sm py-3xs text-2xs text-faint" role="status" aria-live="polite">
          {t(S.projects.switching)}
        </p>
      </Show>
    </div>
  );
}
