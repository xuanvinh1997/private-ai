import "katex/dist/katex.min.css";
import { S, t } from "./i18n";

/** KaTeX wrapper: lazily imported (~300 KB), CSS and fonts bundled for offline use, `trust: false` to block URL macros. */

export type KatexModule = typeof import("katex").default;

let pending: Promise<KatexModule> | null = null;

/** Loaded once per session; on failure, allow a retry at the next formula. */
export function loadKatex(): Promise<KatexModule> {
  if (pending === null) {
    pending = import("katex")
      .then((mod) => mod.default)
      .catch((err) => {
        pending = null;
        throw err;
      });
  }
  return pending;
}

/** Render a formula into a node; returns an error message, or `null` on success. Never throws. */
export function renderMath(
  katex: KatexModule,
  host: HTMLElement,
  tex: string,
  display: boolean,
): string | null {
  try {
    katex.render(tex, host, {
      displayMode: display,
      // Throw so we can render the failure ourselves; KaTeX's own error line never says where the TeX is wrong.
      throwOnError: true,
      trust: false,
      // "warn" is the default and floods the console whenever the model types an accented character in math mode.
      strict: false,
    });
    return null;
  } catch (err) {
    if (err instanceof Error && err.message !== "") return err.message;
    return t(S.libs.math.parseFailed);
  }
}
