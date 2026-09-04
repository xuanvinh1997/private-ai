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

/** Left sidebar; the storage key was renamed with the column, which now holds every route, not just sessions. */
export const [sidebarOpen, setSidebarOpen] = flag("pai-sidebar", true);
/** Right inspector. Keeps the old key so anyone who had the changes panel open keeps their layout. */
export const [workspacePanelOpen, setWorkspacePanelOpen] = flag("pai-changes-panel", false);

const isToolScope = (raw: string): raw is ToolScope =>
  raw === "read" || raw === "write" || raw === "shell";

/** Tool scope a *new* turn starts at; the composer picker still overrides it per turn and always shows the level. */
export const [defaultToolScope, setDefaultToolScope] = persisted<ToolScope>(
  "pai-tool-scope-mac-dinh",
  "write",
  isToolScope,
);
