import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../lib/agent";

/**
 * The three core commands the settings screens use, kept here because only they call them.
 * `describe_harness` returns the human-readable plugin tree, saying which rows are mounted and
 * which config layers touched them; `sandbox_status` and `list_hooks` fill the two gaps.
 */

/** One element per line; it throws when the core cannot build the tree, see `createResource`. */
export async function describeHarness(): Promise<string[]> {
  // Outside Tauri there is no core to ask; return empty rather than throw, as that is normal.
  if (!inTauri()) return [];
  return await invoke<string[]>("describe_harness");
}

/** One row of the plugin tree: `id`, plugin name, and the trail of config layers that touched it. */
export interface HarnessRow {
  id: string;
  plugin: string;
  /** The base layer alone, or the base layer followed by each patch file that edited the row. */
  origin: string;
  disabled: boolean;
}

/** Parse the core's dump into rows; it is two lines per row, so changing the Rust format empties this. */
export function docCayPlugin(lines: string[]): HarnessRow[] {
  const rows: HarnessRow[] = [];
  for (let at = 0; at < lines.length; at += 1) {
    const head = lines[at] ?? "";
    // An indented line is a trail line, not a row; blank and `#` lines fall out here too.
    if (head === "" || head.startsWith(" ") || head.startsWith("#")) continue;
    const split = head.indexOf(": ");
    if (split < 0) continue;
    const id = head.slice(0, split);
    const rest = head.slice(split + 2);
    const disabled = rest.endsWith(" [tắt]");
    const next = lines[at + 1] ?? "";
    rows.push({
      id,
      plugin: disabled ? rest.slice(0, -" [tắt]".length) : rest,
      origin: next.trimStart().startsWith("# ") ? next.trimStart().slice(2) : "",
      disabled,
    });
  }
  return rows;
}

/** Has a user config layer touched this row. */
export function daVa(row: HarnessRow | undefined): boolean {
  return row !== undefined && row.origin.includes("→");
}


/** Process confinement, as the core reports it. */
export interface SandboxStatus {
  /** `full`, `partial` or `none`. */
  mode: string;
  /** Why it leaks, or why there is none; `null` when `full`. */
  reason: string | null;
  writableRoots: string[];
  platform: string;
}

/** The real confinement level; `null` means unanswerable, which the screen must not draw as `none`. */
export async function sandboxStatus(): Promise<SandboxStatus | null> {
  if (!inTauri()) return null;
  try {
    return await invoke<SandboxStatus>("sandbox_status");
  } catch (err) {
    console.error("could not read the sandbox status", err);
    return null;
  }
}

export interface HookRow {
  command: string;
  /** Empty means it applies to every tool. */
  tools: string[];
  timeoutSecs: number | null;
  /** The config layer that declared it. */
  origin: string;
}

/** Installed hooks; an empty list means none are installed, which is the default. */
export async function listHooks(): Promise<HookRow[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<HookRow[]>("list_hooks");
  } catch (err) {
    console.error("could not read the hook list", err);
    return [];
  }
}

/** The real patch file path, honouring `PAI_DATA_DIR`; a default is used outside Tauri. */
export async function hookConfigPath(): Promise<string> {
  if (!inTauri()) return "~/.private-ai/patch.yaml";
  try {
    return await invoke<string>("hook_config_path");
  } catch {
    return "~/.private-ai/patch.yaml";
  }
}
