import { createSignal, type Accessor } from "solid-js";
import type { ToolScope } from "./protocol";

/** Display preferences persisted in localStorage, which throws in private windows, so every access falls back. */

/** `bubble`: chat bubbles on both sides. `document`: full width, like a document. */
export type DisplayMode = "bubble" | "document";

function persisted<T extends string>(
  key: string,
  fallback: T,
  isValid: (raw: string) => raw is T,
): [Accessor<T>, (value: T) => void] {
  let initial = fallback;
  try {
    const raw = localStorage.getItem(key);
    if (raw !== null && isValid(raw)) initial = raw;
  } catch {
    /* ignore */
  }
  const [get, set] = createSignal<T>(initial);
  return [
    get,
    (value: T) => {
      set(() => value);
      try {
        localStorage.setItem(key, value);
      } catch {
        /* cannot persist: the choice lives only for this session */
      }
    },
  ];
}

const isDisplayMode = (raw: string): raw is DisplayMode =>
  raw === "bubble" || raw === "document";

export const [displayMode, setDisplayMode] = persisted<DisplayMode>(
  "pai-display-mode",
  "bubble",
  isDisplayMode,
);

const isFlag = (raw: string): raw is "on" | "off" => raw === "on" || raw === "off";

function flag(key: string, fallback: boolean): [Accessor<boolean>, (value: boolean) => void] {
  const [get, set] = persisted<"on" | "off">(key, fallback ? "on" : "off", isFlag);
  return [() => get() === "on", (value: boolean) => set(value ? "on" : "off")];
}

function numberPreference(
  key: string,
  fallback: number,
  min: number,
  max: number,
): [Accessor<number>, (value: number) => void] {
  let initial = fallback;
  try {
    const value = Number(localStorage.getItem(key));
    if (Number.isFinite(value) && value >= min && value <= max) initial = value;
  } catch {
    /* ignore */
  }
  const [get, set] = createSignal(initial);
  return [
    get,
    (value) => {
      const next = Math.min(max, Math.max(min, Math.round(value)));
      set(next);
      try {
        localStorage.setItem(key, String(next));
      } catch {
        /* cannot persist: the choice lives only for this session */
      }
    },
  ];
}

/** Left sidebar; the storage key was renamed with the column, which now holds every route, not just sessions. */
export const [sidebarOpen, setSidebarOpen] = flag("pai-sidebar", true);
/** Right inspector. Keeps the old key so anyone who had the changes panel open keeps their layout. */
export const [workspacePanelOpen, setWorkspacePanelOpen] = flag("pai-changes-panel", false);

export const SIDEBAR_WIDTH = { min: 220, max: 420, default: 268 } as const;
export const WORKSPACE_PANEL_WIDTH = { min: 252, max: 560, default: 300 } as const;
export const [sidebarWidth, setSidebarWidth] = numberPreference(
  "pai-sidebar-width",
  SIDEBAR_WIDTH.default,
  SIDEBAR_WIDTH.min,
  SIDEBAR_WIDTH.max,
);
export const [workspacePanelWidth, setWorkspacePanelWidth] = numberPreference(
  "pai-workspace-panel-width",
  WORKSPACE_PANEL_WIDTH.default,
  WORKSPACE_PANEL_WIDTH.min,
  WORKSPACE_PANEL_WIDTH.max,
);

const isToolScope = (raw: string): raw is ToolScope =>
  raw === "read" || raw === "write" || raw === "shell";

/** Tool scope a *new* turn starts at; the composer picker still overrides it per turn and always shows the level. */
export const [defaultToolScope, setDefaultToolScope] = persisted<ToolScope>(
  "pai-tool-scope-mac-dinh",
  "write",
  isToolScope,
);
