import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { demoPaths, isDemo } from "./demo";
import { S, t } from "./i18n";
import type { Msg } from "./i18n";

/** The composer's two completions: `@` for files, scored in Rust to keep the file table off the IPC boundary,
 * and `/` for commands, a fixed list that lives here and costs no call. */

/** A command typeable after `/`. */
export interface Command {
  name: string;
  /** Translated hint shown next to the name in the list. */
  hint: string;
  /** Whether an open project is required; such commands still show, disabled with a reason, rather than hiding. */
  needsProject?: boolean;
}

/** A command as declared, hint still untranslated: translating at call time lets the language change mid-session. */
interface CommandDef {
  name: string;
  hint: Msg;
  needsProject?: boolean;
}

/** Command vocabulary; every entry must also be reachable another way, since the palette is a shortcut, not a back door. */
export const COMMANDS: CommandDef[] = [
  { name: "moi", hint: S.libs.command.newSession },
  { name: "tim", hint: S.libs.command.findSession },
  { name: "duan", hint: S.libs.command.projects },
  { name: "thaydoi", hint: S.libs.command.changes, needsProject: true },
  { name: "taplieu", hint: S.libs.command.docs, needsProject: true },
  { name: "mohinh", hint: S.libs.command.models },
  { name: "mcp", hint: S.libs.command.mcp },
  { name: "quyen", hint: S.libs.command.permissions },
  { name: "phimtat", hint: S.libs.command.shortcuts },
  { name: "caidat", hint: S.libs.command.settings },
];

/** A translated command, ready for the UI. */
function resolve(def: CommandDef): Command {
  return {
    name: def.name,
    hint: t(def.hint),
    ...(def.needsProject === true ? { needsProject: true } : {}),
  };
}

/** Rank commands by name prefix, then name substring, then hint text; ties break alphabetically, not by declaration order. */
export function rankCommands(query: string): Command[] {
  const needle = query.trim().toLowerCase();
  const all = COMMANDS.map(resolve);
  if (needle === "") return all;

  const scored: { command: Command; score: number }[] = [];
  for (const command of all) {
    const name = command.name.toLowerCase();
    const hint = command.hint.toLowerCase();
    let score: number | null = null;
    if (name.startsWith(needle)) score = 3;
    else if (name.includes(needle)) score = 2;
    else if (hint.includes(needle)) score = 1;
    if (score !== null) scored.push({ command, score });
  }
  return scored
    .sort((a, b) => b.score - a.score || a.command.name.localeCompare(b.command.name))
    .map((entry) => entry.command);
}

/** Paths matching what was typed after `@`; errors return an empty list, since suggestions must never interrupt typing. */
export async function completePaths(query: string, limit = 8): Promise<string[]> {
  if (isDemo() || !inTauri()) return demoPaths(query, limit);
  try {
    return await invoke<string[]>("complete_paths", { query, limit });
  } catch (err) {
    console.error("failed to complete paths", err);
    return [];
  }
}

/** What is half-typed at the caret, when it is a completion trigger. */
export interface Trigger {
  kind: "path" | "command";
  /** The text after the sigil, up to the caret. */
  query: string;
  /** Index of the sigil (`@` or `/`) in the string. */
  start: number;
  /** Caret position, i.e. the end of what has been typed. */
  end: number;
}

/** Find the completion trigger at the caret: `@` at any word start, `/` only as the first character of the input. */
export function findTrigger(text: string, caret: number): Trigger | null {
  const upto = text.slice(0, caret);

  // Commands: the whole input must start with `/` and contain no whitespace yet.
  if (upto.startsWith("/")) {
    const query = upto.slice(1);
    if (!/\s/.test(query)) return { kind: "command", query, start: 0, end: caret };
    return null;
  }

  const at = upto.lastIndexOf("@");
  if (at < 0) return null;
  const before = at === 0 ? "" : upto[at - 1]!;
  if (before !== "" && !/\s/.test(before)) return null;
  const query = upto.slice(at + 1);
  if (/\s/.test(query)) return null;
  return { kind: "path", query, start: at, end: caret };
}

/** Replace the half-typed trigger with the chosen value and report the new caret; a trailing space is appended. */
export function applyCompletion(
  text: string,
  trigger: Trigger,
  value: string,
): { text: string; caret: number } {
  const inserted = `${value} `;
  const next = text.slice(0, trigger.start) + inserted + text.slice(trigger.end);
  return { text: next, caret: trigger.start + inserted.length };
}
