import { S, t } from "./i18n";
import type { DiffHunk } from "./protocol";

/** Stacked-block diffs, not unified: no side-by-side at ~720px, no `+`/`-` prefixes on screen, old lines then new. */

export type DiffRow =
  | { kind: "path"; text: string; oldNo: null; newNo: null }
  | { kind: "del" | "add"; text: string; oldNo: number | null; newNo: number | null }
  | { kind: "gap"; text: string; oldNo: null; newNo: null };

/** Split into lines, dropping the empty last line left by a trailing `\n`. */
function lines(text: string): string[] {
  const out = text.split("\n");
  if (out.length > 1 && out[out.length - 1] === "") out.pop();
  return out;
}

export function diffTotals(diffs: DiffHunk[]): { added: number; removed: number; files: number } {
  let added = 0;
  let removed = 0;
  const files = new Set<string>();
  for (const hunk of diffs) {
    files.add(hunk.path);
    added += lines(hunk.new_text).length;
    if (hunk.old_text !== null) removed += lines(hunk.old_text).length;
  }
  return { added, removed, files: files.size };
}

export function diffRows(diffs: DiffHunk[]): DiffRow[] {
  const rows: DiffRow[] = [];
  let previousPath: string | null = null;
  for (const hunk of diffs) {
    // A following hunk in the same file only needs an ellipsis; repeating a long path pushes content below the fold.
    rows.push({
      kind: "path",
      text: hunk.path === previousPath ? "⋯" : hunk.path,
      oldNo: null,
      newNo: null,
    });
    previousPath = hunk.path;

    let oldNo = hunk.old_start ?? 1;
    if (hunk.old_text !== null) {
      for (const text of lines(hunk.old_text)) {
        rows.push({ kind: "del", text, oldNo: oldNo++, newNo: null });
      }
    }
    let newNo = hunk.new_start ?? 1;
    for (const text of lines(hunk.new_text)) {
      rows.push({ kind: "add", text, oldNo: null, newNo: newNo++ });
    }
  }
  return rows;
}

/** Fold out the middle when the block is too tall; the head rounds up, since that is the part actually read. */
export function foldRows(rows: DiffRow[], maxLines: number): DiffRow[] {
  if (rows.length <= maxLines) return rows;
  const head = Math.ceil(maxLines / 2);
  const tail = maxLines - head;
  const hidden = rows.length - head - tail;
  return [
    ...rows.slice(0, head),
    { kind: "gap", text: t(S.libs.diff.gap, { n: hidden }), oldNo: null, newNo: null } satisfies DiffRow,
    ...(tail > 0 ? rows.slice(rows.length - tail) : []),
  ];
}

/** Text for the Copy button, the *only* place `+`/`-` prefixes are produced: pasted output should be a unified diff. */
export function diffToText(diffs: DiffHunk[]): string {
  const parts: string[] = [];
  for (const hunk of diffs) {
    parts.push(`--- ${hunk.old_text === null ? "/dev/null" : hunk.path}`);
    parts.push(`+++ ${hunk.path}`);
    if (hunk.old_text !== null) for (const line of lines(hunk.old_text)) parts.push(`-${line}`);
    for (const line of lines(hunk.new_text)) parts.push(`+${line}`);
  }
  return parts.join("\n");
}

/** *Intended* diff, inferred from tool arguments mid-run; `meta.diffs` replaces it once `tool/result` arrives. */
export function intendedDiffs(name: string, args: unknown): DiffHunk[] | null {
  if (args === null || typeof args !== "object") return null;
  const bag = args as Record<string, unknown>;
  const path = typeof bag.file_path === "string" ? bag.file_path : null;
  if (path === null) return null;

  if (name === "write" && typeof bag.content === "string") {
    return [{ path, old_text: null, new_text: bag.content }];
  }
  if (name === "edit" && typeof bag.new_string === "string") {
    const old = typeof bag.old_string === "string" && bag.old_string !== "" ? bag.old_string : null;
    return [{ path, old_text: old, new_text: bag.new_string }];
  }
  return null;
}
