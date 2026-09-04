import { Key } from "@solid-primitives/keyed";
import { createMemo, createSignal, For, Show, type JSX } from "solid-js";
import { S, t, type Msg } from "../lib/i18n";
import type { ProjectKind, SessionSummary } from "../lib/protocol";
import { groupSessions, relativeTime } from "../lib/sessions";
import { setTheme, theme, type ThemeChoice } from "../lib/theme";
import { BrandLockup } from "./Brand";
import Icon, { type IconName } from "./Icon";
import Menu from "./Menu";
import { IconButton } from "./primitives";

/** The open screen. This list is short by decision, not omission: users already have their own editor. */
export type TabId = "chat" | "diff" | "library" | "projects" | "settings";

/** Screens that exist only *inside a project*, and which project kind sees them; they are indented under the open
 * project so they read as belonging to it, and the other kind's entry is removed rather than dimmed. */
const PROJECT_TABS: { id: TabId; label: Msg; icon: IconName; kinds: ProjectKind[] }[] = [
  { id: "diff", label: S.chat.sidebar.tabChanges, icon: "diff", kinds: ["code"] },
  { id: "library", label: S.chat.sidebar.tabDocs, icon: "library", kinds: ["docs"] },
];

/** Sub-entries of the open project; with no project (`kind` absent) there are none. */
export function projectTabs(
  kind: ProjectKind | undefined,
): { id: TabId; label: string; icon: IconName }[] {
  return PROJECT_TABS.filter((item) => kind !== undefined && item.kinds.includes(kind)).map(
    (item) => ({ id: item.id, label: t(item.label), icon: item.icon }),
  );
}

/** Screens reachable for a project kind; `App` uses it to fix the current screen when the project changes. */
export function tabsFor(kind: ProjectKind | undefined): TabId[] {
  return ["chat", ...projectTabs(kind).map((item) => item.id), "projects", "settings"];
}

const NEXT_THEME: Record<ThemeChoice, ThemeChoice> = {
  light: "dark",
  dark: "system",
  system: "light",
};

const THEME_ICON: Record<ThemeChoice, IconName> = {
  light: "sun",
  dark: "moon",
  system: "monitor",
};

const THEME_LABEL: Record<ThemeChoice, Msg> = {
  light: S.chat.sidebar.themeLight,
  dark: S.chat.sidebar.themeDark,
  system: S.chat.sidebar.themeSystem,
};

export interface SidebarProps {
  sessions: SessionSummary[];
  currentId: string;
  loading: boolean;
  /** The open screen, so its row carries `aria-current`. */
  view: TabId;
  /** Number of connected MCP servers, used as the badge on that row. */
  mcpCount?: number;
  /** The "Projects" group, passed as JSX so this column need not know the project contract. A *function*, not a
   * `JSX.Element`: Solid compiles a JSX prop into a getter, so reading it twice builds two live components. */
  projectsSlot?: () => JSX.Element;
  /** Switching projects: the list below still describes the project about to close. */
  disabled?: boolean;
  /** Secondary line per session: the last thing said. Absent, the line is dropped. */
  subtitle?: (session: SessionSummary) => string | undefined;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  onGo: (view: TabId) => void;
  /** MCP servers, this app's plugins; links straight to the MCP page in Settings. */
  onOpenMcp: () => void;
  onCollapse: () => void;
}

/** The sidebar: the only left column, and the whole navigation system. Ordered to tell a story - which app, what
 * you can do now, projects, what was done, then configuration at the foot. Group headings are labels, not buttons.
 * The session filter hides behind a magnifier rather than occupying a permanent row. */
export default function Sidebar(props: SidebarProps) {
  const [query, setQuery] = createSignal("");
  const [searching, setSearching] = createSignal(false);
  // A row with an open menu keeps its trigger visible after the pointer leaves, or the menu floats beside nothing.
  const [menuOn, setMenuOn] = createSignal<string | null>(null);
  let searchField: HTMLInputElement | undefined;

  const toggleSearch = () => {
    const next = !searching();
    setSearching(next);
    // Closing the filter while keeping the query would hide why the list is down to three rows.
    if (!next) setQuery("");
    else queueMicrotask(() => searchField?.focus());
  };

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (needle === "") return props.sessions;
    return props.sessions.filter((session) => session.title.toLowerCase().includes(needle));
  });

  const groups = createMemo(() => groupSessions(matches()));

  return (
    <aside
      aria-label={t(S.chat.sidebar.nav)}
      class="flex w-(--sidebar-w) shrink-0 flex-col border-r border-line bg-sidebar"
    >
      {/* Window drag strip and the space for the macOS traffic lights; it stays empty because they sit on top. */}
      <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

      {/* Top row: brand on the left, two small buttons on the right. `pb-xs` so it reads as a header, not a row. */}
      <div class="flex shrink-0 items-center gap-2xs px-sm pb-xs">
        <BrandLockup class="flex-1" />
        <IconButton
          icon="search"
          label={t(searching() ? S.chat.sidebar.searchClose : S.chat.sidebar.searchOpen)}
          size="sm"
          active={searching()}
          expanded={searching()}
          onClick={toggleSearch}
        />
        <IconButton
          icon="panel-left"
          label={t(S.chat.sidebar.collapse)}
          size="sm"
          onClick={props.onCollapse}
        />
      </div>

      <Show when={searching()}>
        <div class="shrink-0 px-sm pb-2xs">
          <input
            ref={searchField}
            type="search"
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                toggleSearch();
              }
            }}
            placeholder={t(S.chat.sessionSearch)}
            aria-label={t(S.chat.sidebar.searchField)}
            disabled={props.disabled}
            class="h-(--control-h) w-full rounded-btn border border-line-strong bg-surface px-sm text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
          />
        </div>
      </Show>

      {/* One scroll region down to the end of "Recent": two nested scrollbars in a 260px column are unhittable. */}
      <div
        class="flex min-h-0 flex-1 flex-col overflow-y-auto transition-opacity duration-[var(--dur-base)]"
        aria-busy={props.disabled}
        classList={{ "pointer-events-none opacity-40": props.disabled === true }}
      >
        <nav aria-label={t(S.chat.sidebar.navMain)} class="shrink-0 px-sm pb-sm">
          <ul class="m-0 flex list-none flex-col gap-3xs p-0">
            <li>
              <NavRow
                icon="plus"
                label={t(S.chat.sidebar.newSession)}
                disabled={props.disabled}
                onClick={props.onCreate}
              />
            </li>
            <li>
              {/* MCP servers are this app's "Plugins": each one adds a basket of tools to the assistant. */}
              <NavRow
                icon="plug"
                label={t(S.chat.sidebar.mcp)}
                badge={props.mcpCount ?? 0}
                disabled={props.disabled}
                onClick={props.onOpenMcp}
              />
            </li>
          </ul>
        </nav>

        <Show when={props.projectsSlot}>
          {(slot) => (
            <section class="shrink-0 px-sm pb-sm">
              <GroupTitle>{t(S.chat.sidebar.projects)}</GroupTitle>
              {slot()()}
            </section>
          )}
        </Show>

        <section class="shrink-0 px-sm pb-md">
          <GroupTitle>{t(S.chat.sidebar.recent)}</GroupTitle>
          <Show when={!props.loading} fallback={<SessionSkeleton />}>
            <Show
              when={groups().length > 0}
              fallback={
                <p class="m-0 flex items-center gap-2xs px-sm py-xs text-2xs text-faint">
                  <Icon name={props.sessions.length === 0 ? "bubble" : "search"} size={12} />
                  {t(
                    props.sessions.length === 0 ? S.chat.sidebar.noSessions : S.chat.noSessionMatch,
                  )}
                </p>
              }
            >
              <For each={groups()}>
                {(group) => (
                  <div class="mb-xs">
                    {/* Date groups sit under "Recent" one size down; equal sizes would read as sibling groups. */}
                    <h3 class="sticky top-0 z-10 m-0 bg-sidebar px-sm py-3xs text-2xs font-semibold tracking-wide text-faint">
                      {group.label}
                    </h3>
                    <ul class="m-0 flex list-none flex-col gap-3xs p-0">
                      {/* Keyed by `id`: the list reorders after every turn, and index keys would drop keyboard focus. */}
                      <Key each={group.sessions} by="id">
                        {(session) => (
                          <SessionRow
                            session={session()}
                            active={props.view === "chat" && session().id === props.currentId}
                            subtitle={props.subtitle?.(session())}
                            menuOpen={menuOn() === session().id}
                            onMenuChange={(open) => setMenuOn(open ? session().id : null)}
                            onSelect={() => props.onSelect(session().id)}
                            onRename={() => props.onRename(session().id)}
                            onDelete={() => props.onDelete(session().id)}
                          />
                        )}
                      </Key>
                    </ul>
                  </div>
                )}
              </For>
            </Show>
          </Show>
        </section>
      </div>

      {/* Foot of the column, one row: settings plus the light/dark toggle, both touched a few times a week. */}
      <footer class="flex shrink-0 items-center gap-2xs border-t border-line p-sm">
        <span class="min-w-0 flex-1">
          <NavRow
            icon="settings"
            label={t(S.common.settings)}
            active={props.view === "settings"}
            onClick={() => props.onGo("settings")}
          />
        </span>
        <IconButton
          icon={THEME_ICON[theme()]}
          label={t(S.chat.sidebar.themeToggle, { name: t(THEME_LABEL[theme()]) })}
          size="sm"
          onClick={() => setTheme(NEXT_THEME[theme()])}
        />
      </footer>
    </aside>
  );
}

/** A group heading: text, not a button, since everything clickable in this column leads somewhere. */
function GroupTitle(props: { children: JSX.Element }) {
  return (
    <h2 class="m-0 px-sm py-2xs text-2xs font-medium text-faint">{props.children}</h2>
  );
}

/** One navigation row; the icon is `aria-hidden` and the meaning travels through the button's own label. */
function NavRow(props: {
  icon: IconName;
  label: string;
  hint?: string;
  active?: boolean;
  disabled?: boolean;
  badge?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={props.onClick}
      disabled={props.disabled}
      aria-current={props.active ? "page" : undefined}
      aria-label={props.hint === undefined ? undefined : `${props.label}. ${props.hint}.`}
      class="flex w-full items-center gap-sm rounded-panel px-sm py-2xs text-left text-sm text-text transition-colors duration-[var(--dur-fast)] disabled:cursor-not-allowed disabled:opacity-40 enabled:hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft aria-[current=page]:font-medium aria-[current=page]:text-accent-ink"
    >
      <span class="shrink-0 text-muted">
        <Icon name={props.icon} size={16} />
      </span>
      <span class="min-w-0 flex-1 truncate">{props.label}</span>
      {/* The counts must find the eye: otherwise they only exist if someone remembers to open that screen. */}
      <Show when={(props.badge ?? 0) > 0}>
        <span class="shrink-0 rounded-pill bg-accent px-2xs text-2xs leading-4 text-on-accent tabular-nums">
          {props.badge}
        </span>
      </Show>
    </button>
  );
}

function SessionRow(props: {
  session: SessionSummary;
  active: boolean;
  subtitle?: string;
  menuOpen: boolean;
  onMenuChange: (open: boolean) => void;
  onSelect: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  return (
    <li
      class="group relative"
      // Right-click opens the same menu as the trigger: two entrances, one menu, no action exclusive to either.
      onContextMenu={(event) => {
        event.preventDefault();
        props.onMenuChange(true);
      }}
    >
      <button
        type="button"
        onClick={props.onSelect}
        aria-current={props.active ? "page" : undefined}
        class="flex w-full flex-col items-start gap-3xs rounded-panel px-sm py-2xs pr-(--sp-3xl) text-left transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] aria-[current=page]:bg-accent-soft"
      >
        <span class="flex w-full min-w-0 items-baseline gap-sm">
          <span
            class="min-w-0 flex-1 truncate text-sm"
            classList={{
              "text-text": !props.active,
              "font-medium text-accent-ink": props.active,
            }}
          >
            {props.session.title}
          </span>
          <span class="shrink-0 text-2xs whitespace-nowrap text-faint tabular-nums">
            {relativeTime(props.session.updatedAt)}
          </span>
        </span>
        <Show when={props.subtitle}>
          {(text) => <span class="w-full truncate text-2xs text-muted">{text()}</span>}
        </Show>
      </button>

      <div
        class="absolute top-2xs right-2xs transition-opacity duration-[var(--dur-fast)] group-hover:opacity-100 group-focus-within:opacity-100"
        classList={{ "opacity-0": !props.menuOpen, "opacity-100": props.menuOpen }}
      >
        <Menu
          label={t(S.chat.sidebar.rowMenu, { title: props.session.title })}
          open={props.menuOpen}
          onOpenChange={props.onMenuChange}
          onRequestClose={() => props.onMenuChange(false)}
          items={[
            { id: "rename", label: t(S.common.rename), icon: "document", onSelect: props.onRename },
            {
              id: "delete",
              label: t(S.chat.sidebar.deleteSession),
              icon: "trash",
              danger: true,
              onSelect: props.onDelete,
            },
          ]}
        />
      </div>
    </li>
  );
}

/** Loading skeleton, at the real row height, or the layout jumps when the data arrives. */
function SessionSkeleton(props: { rows?: number }) {
  return (
    <div class="flex flex-col gap-2xs px-sm" aria-hidden="true">
      <For each={Array.from({ length: props.rows ?? 6 })}>
        {(_, index) => (
          <div class="flex flex-col gap-2xs py-2xs">
            <div class="flex items-center gap-sm">
              <div
                class="h-3 rounded-pill bg-[var(--overlay-hover)] motion-safe:animate-pulse"
                style={{ width: `${[68, 82, 55, 74, 60, 88][index() % 6]}%` }}
              />
            </div>
            <div class="h-2.5 w-2/5 rounded-pill bg-[var(--overlay-faint)] motion-safe:animate-pulse" />
          </div>
        )}
      </For>
    </div>
  );
}
