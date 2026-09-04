import { Show } from "solid-js";
import { S, t } from "../lib/i18n";
import { BrandMark } from "./Brand";
import NotificationBell from "./NotificationBell";
import { IconButton } from "./primitives";

/** Workspace top bar, deliberately thin: title, turn status, and one button. The project name is a breadcrumb
 * shown only when a project is open. With the sidebar collapsed the brand mark moves here. The whole strip is
 * the window drag region, since the window runs in "Overlay" mode. */
export default function WorkspaceHeader(props: {
  title: string;
  /** The open project; absent means no project is open, which is not the same as an empty string. */
  scope?: string;
  busy: boolean;
  /** Text beside the title while busy; defaults to the running turn. */
  busyLabel?: string;
  /** Whether the sidebar is open; when closed, this bar must reserve room for the macOS traffic lights. */
  sidebarOpen: boolean;
  onOpenSidebar: () => void;
  /** Changes panel toggle; absent means this screen has no panel to open. */
  changesPanelOpen?: boolean;
  changeCount?: number;
  onToggleChangesPanel?: () => void;
}) {
  return (
    <header
      class="flex h-(--header-h) shrink-0 items-center gap-sm bg-bg px-md"
      classList={{ "pl-(--traffic-lights-w)": !props.sidebarOpen }}
      data-tauri-drag-region
    >
      <Show when={!props.sidebarOpen}>
        <IconButton
          icon="panel-left"
          label={t(S.chat.header.openSidebar)}
          onClick={props.onOpenSidebar}
        />
        <BrandMark size={22} class="shrink-0 text-accent" />
      </Show>

      <div class="flex min-w-0 flex-1 items-baseline gap-xs">
        {/* The project name gives way first: truncating the title would cut the thing the user just opened. */}
        <Show when={props.scope}>
          {(scope) => (
            <>
              <span class="min-w-0 max-w-40 shrink truncate text-sm text-muted">{scope()}</span>
              <span class="shrink-0 text-sm text-faint" aria-hidden="true">
                /
              </span>
            </>
          )}
        </Show>
        {/* `text-lg`, not `text-base`: at body size, the only line in a 56px bar reads as a stray label. */}
        <h1 class="m-0 min-w-0 truncate text-lg font-medium text-ink">{props.title}</h1>
        <Show when={props.busy}>
          <span class="shrink-0 text-2xs text-accent" role="status" aria-live="polite">
            {props.busyLabel ?? t(S.chat.header.busy)}
          </span>
        </Show>
      </div>

      <NotificationBell />

      <Show when={props.onToggleChangesPanel}>
        {(toggle) => (
          <span class="relative inline-flex">
            <IconButton
              icon="panel-right"
              label={
                props.changesPanelOpen
                  ? t(S.chat.changes.close)
                  : (props.changeCount ?? 0) > 0
                    ? t(S.chat.header.openChangesCount, { n: props.changeCount ?? 0 })
                    : t(S.chat.header.openChanges)
              }
              active={props.changesPanelOpen}
              onClick={() => toggle()()}
            />
            {/* The changed-file count sits on the button, because the panel is usually closed. */}
            <Show when={(props.changeCount ?? 0) > 0 && props.changesPanelOpen !== true}>
              <span
                aria-hidden="true"
                class="pointer-events-none absolute -top-2xs -right-2xs grid min-w-5 place-items-center rounded-pill border-2 border-bg bg-accent px-3xs text-2xs leading-4 text-on-accent tabular-nums"
              >
                {props.changeCount}
              </span>
            </Show>
          </span>
        )}
      </Show>
    </header>
  );
}
