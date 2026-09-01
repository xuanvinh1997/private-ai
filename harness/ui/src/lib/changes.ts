import { diffTotals } from "./diff";
import type { ConversationNode, DiffHunk } from "./protocol";

export interface ChangedFile {
  path: string;
  added: number;
  removed: number;
  /** Node cuối cùng đụng vào tệp này — đích của cú bấm "cuộn tới thay đổi". */
  nodeId: string;
  /** Tệp mới hoàn toàn: mọi hunk đều không có bản cũ. */
  created: boolean;
  /** Diff mới chỉ là *dự kiến*: tool chưa chạy xong, tệp trên đĩa chưa đổi. */
  pending: boolean;
}

/**
 * Danh sách tệp đã đổi trong phiên, gấp lại từ chính bản ghi hội thoại.
 *
 * Không có sự kiện riêng nào cho việc này và cũng không nên có: bản ghi đã mang đủ dữ
 * liệu, còn một sự kiện thứ hai nói cùng một chuyện là một cơ hội để hai chỗ lệch nhau.
 *
 * Neo cuộn trỏ tới lần đụng **gần nhất** chứ không phải lần đầu — người dùng bấm vào một
 * tệp là để xem nó đang thành cái gì, không phải để xem nó từng là cái gì.
 */
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
        // Cộng dồn: hai lần `edit` trên cùng một tệp là hai lần thêm/xoá, không phải một.
        added: (previous?.added ?? 0) + totals.added,
        removed: (previous?.removed ?? 0) + totals.removed,
        nodeId: node.id,
        created: (previous?.created ?? true) && forFile.every((hunk) => hunk.old_text === null),
        pending,
      });
    }
  }

  return [...byPath.values()];
}

/** Tên tệp không kèm thư mục — phần duy nhất phân biệt được trong một cột hẹp. */
export function baseName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

/** Thư mục chứa, để dưới tên tệp làm dòng phụ. */
export function dirName(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut <= 0 ? "" : path.slice(0, cut);
}
