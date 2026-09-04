import { createSignal } from "solid-js";

export type ThemeChoice = "light" | "dark" | "system";

const STORAGE_KEY = "pai-theme";

function read(): ThemeChoice {
  // localStorage throws in private windows and when site data is blocked; fall back to "system".
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    /* ignore */
  }
  return "system";
}

const [theme, setThemeSignal] = createSignal<ThemeChoice>(read());

/** Stamp the choice on `<html>`; "system" stamps nothing so prefers-color-scheme can apply (theme.py:239). */
function stamp(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
}

export function setTheme(choice: ThemeChoice): void {
  setThemeSignal(choice);
  stamp(choice);
  try {
    localStorage.setItem(STORAGE_KEY, choice);
  } catch {
    /* cannot persist: the choice lives only for this session, which still beats throwing */
  }
}

/** Call once at startup, before render, to avoid a wrong-theme flash. */
export function initTheme(): void {
  stamp(theme());
}

export { theme };
