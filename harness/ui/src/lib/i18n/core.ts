import { createSignal } from "solid-js";

/** i18n core: a message is an object, not a string key, so a wrong key or a missing translation fails to compile.
 * Defaults to `en` and never sniffs `navigator.language`; the display language is an explicit setting. */

export type Locale = "en" | "vi";

/** One message in every locale; a missing locale is a type error. */
export type Msg = Readonly<Record<Locale, string>>;

export const LOCALES: readonly Locale[] = ["en", "vi"] as const;

const STORAGE_KEY = "pai-locale";

const isLocale = (raw: string): raw is Locale => raw === "en" || raw === "vi";

function read(): Locale {
  // localStorage throws in private windows and when site data is blocked; fall back to "en".
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw !== null && isLocale(raw)) return raw;
  } catch {
    /* ignore */
  }
  return "en";
}

const [locale, setLocaleSignal] = createSignal<Locale>(read());

/** Stamp the locale on `<html lang>`; screen readers pick a voice from it and browsers pick line-breaking rules. */
function stamp(next: Locale): void {
  document.documentElement.setAttribute("lang", next);
}

export function setLocale(next: Locale): void {
  setLocaleSignal(next);
  stamp(next);
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    /* cannot persist: the choice lives only for this session */
  }
}

/** Call once at startup, before render. */
export function initLocale(): void {
  stamp(locale());
}

/** Translate a message, filling `{name}` slots; it reads the `locale()` signal, so JSX updates on a language change. */
export function t(msg: Msg, params?: Record<string, string | number>): string {
  const raw = msg[locale()] ?? msg.en;
  if (!params) return raw;
  return raw.replace(/\{(\w+)\}/g, (whole, key: string) =>
    key in params ? String(params[key]) : whole,
  );
}

/** Singular/plural forms, with `{n}` always available; Vietnamese usually repeats one form, which is cheaper than per-call logic. */
export function tn(
  n: number,
  one: Msg,
  other: Msg,
  params?: Record<string, string | number>,
): string {
  return t(n === 1 ? one : other, { n, ...params });
}

export { locale };
