/**
 * Split assistant text into prose segments and backtick-fenced code blocks.
 * It runs before `Markdown.tsx`, because `marked` folds mermaid and unclosed fences into one
 * `code` token and loses the distinction this file has to keep.
 */

import { S, t } from "../../lib/i18n";
import type { Msg } from "../../lib/i18n";

export type Segment =
  | { kind: "text"; text: string }
  /** `closed` is false while the opening fence has no match yet, meaning it is still being typed. */
  | { kind: "fence"; lang: string; code: string; closed: boolean };

/** Opening fence: up to three spaces of indent, three or more backticks, then the info string. */
const OPEN = /^ {0,3}(`{3,})[ \t]*([^`\n]*)$/;
/** Closing fence: same indent rule, nothing after the backticks. */
const CLOSE = /^ {0,3}(`{3,})[ \t]*$/;

export function splitFences(input: string): Segment[] {
  const out: Segment[] = [];
  const lines = input.split("\n");
  let text: string[] = [];

  const flushText = (): void => {
    if (text.length === 0) return;
    const joined = text.join("\n");
    // An empty text segment between two code blocks only adds a stray gap.
    if (joined.trim() !== "") out.push({ kind: "text", text: joined });
    text = [];
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i] ?? "";
    const open = OPEN.exec(line);
    if (open === null) {
      text.push(line);
      continue;
    }

    flushText();
    const ticks = (open[1] ?? "```").length;
    const lang = (open[2] ?? "").trim().split(/\s+/)[0] ?? "";
    const body: string[] = [];
    let closed = false;
    i += 1;
    for (; i < lines.length; i += 1) {
      const inner = lines[i] ?? "";
      const close = CLOSE.exec(inner);
      if (close !== null && (close[1] ?? "").length >= ticks) {
        closed = true;
        break;
      }
      body.push(inner);
    }
    out.push({ kind: "fence", lang: lang.toLowerCase(), code: body.join("\n"), closed });
  }

  flushText();
  return out;
}

/** Language label above a code block; only the two word-like labels are translated, names are not. */
const LANG_MSG: Record<string, Msg> = {
  "": S.tools.code.plain,
  text: S.tools.code.text,
  txt: S.tools.code.text,
};

const LANG_ALIAS: Record<string, string> = {
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  console: "shell",
  rs: "rust",
  py: "python",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  yml: "yaml",
  md: "markdown",
};

export const langLabel = (lang: string): string => {
  const msg = LANG_MSG[lang];
  return msg === undefined ? (LANG_ALIAS[lang] ?? lang) : t(msg);
};
