import { diffTotals } from "./diff";
import type { ConversationNode, DiffHunk } from "./protocol";

export interface ChangedFile {
  path: string;
  added: number;
  removed: number;
  /** Last node that touched this file; the scroll target of "jump to change". */
  nodeId: string;
  /** Brand-new file: no hunk has an old version. */
  created: boolean;
  /** The diff is still *intended*: the tool has not finished and the file on disk is unchanged. */
  pending: boolean;
  /** Every hunk that touched this file, in order; accumulated rather than replaced, so an earlier edit is not hidden. */
  hunks: DiffHunk[];
}

/** Files changed this session, folded out of the transcript itself; the scroll anchor points at the latest touch. */
export function changedFiles(nodes: ConversationNode[]): ChangedFile[] {
  const byPath = new Map<string, ChangedFile>();

  for (const node of nodes) {
    if (node.kind !== "tool") continue;
    if (node.call.state === "error") continue;
    const applied = node.call.meta?.diffs;
    const hunks: DiffHunk[] | undefined =
      applied && applied.length > 0 ? applied : node.call.intendedDiffs;
    if (!hunks || hunks.length === 0) continue;

    const pending = !(applied && applied.length > 0);
    for (const path of new Set(hunks.map((hunk) => hunk.path))) {
      const forFile = hunks.filter((hunk) => hunk.path === path);
      const totals = diffTotals(forFile);
      const previous = byPath.get(path);
      byPath.set(path, {
        path,
        // Accumulate: two `edit` calls on one file are two sets of additions and removals, not one.
        added: (previous?.added ?? 0) + totals.added,
        removed: (previous?.removed ?? 0) + totals.removed,
        nodeId: node.id,
        created: (previous?.created ?? true) && forFile.every((hunk) => hunk.old_text === null),
        pending,
        hunks: [...(previous?.hunks ?? []), ...forFile],
      });
    }
  }

  return [...byPath.values()];
}

/** Filename without its directory: the only part that distinguishes files in a narrow column. */
export function baseName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

/** Containing directory, shown as a secondary line under the filename. */
export function dirName(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut <= 0 ? "" : path.slice(0, cut);
}
