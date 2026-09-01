import type { DiffHunk } from "./protocol";

/**
 * Dựng khối diff dạng *stacked block*, không phải unified diff.
 *
 * Ba luật chép từ bản web của dsh, và cả ba đều có lý do:
 *   1. Không side-by-side — chat rộng ~720px, hai cột thì mỗi cột hết chỗ thở.
 *   2. Không ký tự `+`/`-` ở đầu dòng. Màu nền phân biệt thêm/xoá; ký tự prefix chỉ
 *      tồn tại trong văn bản nút Copy sinh ra. Người dùng bôi đen một dòng trên màn
 *      hình rồi dán vào editor phải ra đúng dòng mã, không ra dòng mã có rác ở đầu.
 *   3. Toàn bộ dòng cũ trước, rồi toàn bộ dòng mới — không trộn xen kẽ. Mắt đọc "trước"
 *      rồi "sau", không phải nhảy qua lại từng dòng.
 */

export type DiffRow =
  | { kind: "path"; text: string; oldNo: null; newNo: null }
  | { kind: "del" | "add"; text: string; oldNo: number | null; newNo: number | null }
  | { kind: "gap"; text: string; oldNo: null; newNo: null };

/** Tách dòng, bỏ dòng rỗng cuối do `\n` kết thúc tệp sinh ra. */
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
    // Hunk kế tiếp trong cùng một tệp chỉ cần dấu ⋯: lặp lại đường dẫn dài ở mỗi hunk
    // đẩy nội dung thật xuống dưới nếp gấp.
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

/**
 * Cắt bớt phần giữa khi khối quá cao. Cùng số học với khối terminal: nửa trên làm tròn
 * lên, phần còn lại xuống dưới — nên với `maxLines` lẻ thì phần đầu dài hơn, và đó là
 * phần người ta đọc thật.
 */
export function foldRows(rows: DiffRow[], maxLines: number): DiffRow[] {
  if (rows.length <= maxLines) return rows;
  const head = Math.ceil(maxLines / 2);
  const tail = maxLines - head;
  const hidden = rows.length - head - tail;
  return [
    ...rows.slice(0, head),
    { kind: "gap", text: `⋯ ẩn ${hidden} dòng`, oldNo: null, newNo: null } satisfies DiffRow,
    ...(tail > 0 ? rows.slice(rows.length - tail) : []),
  ];
}

/**
 * Văn bản cho nút Copy. Đây là chỗ *duy nhất* `+`/`-` được sinh ra: dán vào chỗ khác
 * thì người ta mong một unified diff, còn trên màn hình thì mong mã sạch.
 */
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

/**
 * Diff *dự kiến*, suy từ đối số của tool khi nó còn đang chạy.
 *
 * Người dùng thấy được thay đổi sắp xảy ra trước cả khi tệp bị đụng vào — quan trọng
 * nhất đúng lúc hộp thoại duyệt đang mở. Khi `tool/result` về thì `meta.diffs` (diff
 * đã áp thật) thay chỗ, vì tool có quyền ghi khác điều nó nói.
 */
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
