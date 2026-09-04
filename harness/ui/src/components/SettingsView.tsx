import { createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import Icon from "./Icon";
import { S, t, tn } from "../lib/i18n";
import McpView from "./mcp/McpView";
import ProvidersView from "./providers/ProvidersView";
import GeneralPage from "./settings/GeneralPage";
import HooksPage from "./settings/HooksPage";
import PermissionsPage from "./settings/PermissionsPage";
import ShortcutsPage from "./settings/ShortcutsPage";
import { NAV, pageMeta, timTrongCaiDat, type SettingsPage } from "./settings/muc-luc";

export type { SettingsPage } from "./settings/muc-luc";

/** Settings as a full-window mode, not a workspace tab: a vertical grouped column scales past four pages, and
 * settings is a *place* you leave, so the exit is doubled (a back button and `Esc`). The open page lives in
 * `App`, because local state here would swallow the sidebar's direct link to the MCP page. */
export default function SettingsView(props: {
  page: SettingsPage;
  onPage: (page: SettingsPage) => void;
  /** The exit; `Esc` and the back button both call this. */
  onClose: () => void;
}) {
  let heading: HTMLHeadingElement | undefined;
  const [query, setQuery] = createSignal("");
  const results = createMemo(() => timTrongCaiDat(query()));
  const searching = () => query().trim() !== "";
  const meta = () => pageMeta(props.page);

  const go = (page: SettingsPage) => {
    props.onPage(page);
    // Navigating means the search is done; keeping the query would leave the results covering the page just opened.
    setQuery("");
    queueMicrotask(() => heading?.focus());
  };

  /** `Esc` clears the search, or leaves settings; bound on `window`, and skipped once a dialog has handled it. */
  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (searching()) {
        setQuery("");
        return;
      }
      props.onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  return (
    // `fixed inset-0`, not a grid cell: this screen *is* the window, so both columns reserve `--titlebar-h`.
    <div class="fixed inset-0 z-30 flex bg-bg">
      <aside
        aria-label={t(S.common.settings)}
        class="flex w-(--sidebar-w) shrink-0 flex-col border-r border-line bg-sidebar"
      >
        <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

        <div class="shrink-0 px-sm pb-2xs">
          <button
            type="button"
            onClick={() => props.onClose()}
            class="flex w-full items-center gap-2xs rounded-panel px-2xs py-2xs text-sm font-medium text-ink transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)]"
          >
            <Icon name="chevron-right" size={14} class="rotate-180" />
            {t(S.settings.shell.backToApp)}
          </button>
        </div>

        <div class="shrink-0 px-sm pb-sm">
          <div class="relative">
            {/* The magnifier sits *inside* the field: the search box is always visible, with nothing to toggle. */}
            <span
              aria-hidden="true"
              class="pointer-events-none absolute top-1/2 left-sm -translate-y-1/2 text-faint"
            >
              <Icon name="search" size={13} />
            </span>
            <input
              type="search"
              value={query()}
              onInput={(event) => setQuery(event.currentTarget.value)}
              placeholder={t(S.settings.shell.searchPlaceholder)}
              aria-label={t(S.settings.shell.searchLabel)}
              spellcheck={false}
              autocapitalize="off"
              autocomplete="off"
              class="h-(--control-h) w-full rounded-btn border border-line-strong bg-surface pr-sm pl-2xl text-xs text-text transition-colors duration-[var(--dur-fast)] placeholder:text-faint focus:border-accent"
            />
          </div>
        </div>

        <nav aria-label={t(S.settings.shell.navLabel)} class="min-h-0 flex-1 overflow-y-auto px-sm pb-lg">
          <For each={NAV}>
            {(group) => (
              <div class="mb-md flex flex-col gap-3xs last:mb-0">
                <Show when={group.title}>
                  {(title) => (
                    // Group headings are text, not buttons: they lead nowhere, so they must not look clickable.
                    <h2 class="m-0 px-2xs pt-2xs pb-3xs text-2xs font-medium text-faint">
                      {t(title())}
                    </h2>
                  )}
                </Show>
                <For each={group.pages}>
                  {(item) => (
                    <button
                      type="button"
                      onClick={() => go(item.id)}
                      aria-current={props.page === item.id ? "page" : undefined}
                      class="flex items-center gap-sm rounded-panel px-2xs py-2xs text-left text-xs text-muted transition-colors duration-[var(--dur-fast)] hover:bg-[var(--overlay-hover)] hover:text-ink aria-[current=page]:bg-surface-hover aria-[current=page]:font-medium aria-[current=page]:text-ink"
                    >
                      <Icon name={item.icon} size={15} />
                      <span class="min-w-0 truncate">{t(item.label)}</span>
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>
        </nav>
      </aside>

      <div class="flex min-w-0 flex-1 flex-col">
        <div class="h-(--titlebar-h) shrink-0" data-tauri-drag-region />

        <div class="min-h-0 flex-1 overflow-y-auto px-(--page-pad-x) pb-4xl">
          <div class="mx-auto flex w-full max-w-(--reading-measure) flex-col">
            <Show
              when={!searching()}
              fallback={
                <SearchPane query={query()} hits={results()} onGo={go} />
              }
            >
              {/* The page title is the only thing saying where you are, on a screen where every page looks alike. */}
              <header class="flex flex-col gap-2xs pt-3xl pb-2xl">
                <h1
                  ref={heading}
                  tabIndex={-1}
                  class="m-0 max-w-[18ch] text-display font-medium tracking-[-0.025em] text-ink"
                >
                  {t(meta().label)}
                </h1>
                <Show when={meta().desc}>
                  {(desc) => <p class="m-0 max-w-[60ch] text-sm text-muted">{t(desc())}</p>}
                </Show>
              </header>

              <Switch>
                <Match when={props.page === "chung"}>
                  <GeneralPage />
                </Match>
                <Match when={props.page === "phim-tat"}>
                  <ShortcutsPage />
                </Match>
                {/* One page for both roles: they are assigned from the same provider list, so two pages would repeat it. */}
                <Match when={props.page === "provider"}>
                  <ProvidersView />
                </Match>
                <Match when={props.page === "mcp"}>
                  <McpView />
                </Match>
                <Match when={props.page === "hook"}>
                  <HooksPage />
                </Match>
                <Match when={props.page === "quyen"}>
                  <PermissionsPage />
                </Match>
              </Switch>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Search results; each row names its page, because a settings hit is a route, not an answer. An empty result
 * says so in words, quoting the query, since a bare empty list reads as a half-drawn screen. */
function SearchPane(props: {
  query: string;
  hits: ReturnType<typeof timTrongCaiDat>;
  onGo: (page: SettingsPage) => void;
}) {
  return (
    <>
      <header class="flex flex-col gap-2xs pt-3xl pb-2xl">
        <h1 class="m-0 text-display font-medium tracking-tight text-ink">
          {t(S.settings.search.title)}
        </h1>
        <p class="m-0 text-sm text-muted" role="status" aria-live="polite">
          {props.hits.length === 0
            ? t(S.settings.search.none, { q: props.query.trim() })
            : tn(props.hits.length, S.settings.search.hitOne, S.settings.search.hitMany, {
                q: props.query.trim(),
              })}
        </p>
      </header>

      <Show
        when={props.hits.length > 0}
        fallback={
          <p class="m-0 max-w-[60ch] text-xs text-muted">
            {t(S.settings.search.scope)}
          </p>
        }
      >
        <ul class="m-0 flex list-none flex-col gap-2xs p-0">
          <For each={props.hits}>
            {(hit) => (
              <li>
                <button
                  type="button"
                  onClick={() => props.onGo(hit.page)}
                  class="flex w-full flex-col gap-3xs rounded-card border border-line bg-surface px-(--card-pad-x) py-sm text-left transition-colors duration-[var(--dur-fast)] hover:bg-surface-hover"
                >
                  <span class="flex flex-wrap items-baseline gap-2xs">
                    <span class="text-xs font-medium text-ink">{t(hit.label)}</span>
                    <span class="flex items-center gap-3xs text-2xs text-faint">
                      <Icon name="chevron-right" size={10} />
                      {t(pageMeta(hit.page).label)}
                    </span>
                  </span>
                  <span class="text-2xs text-muted">{t(hit.desc)}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </>
  );
}
