import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { demoPaths, isDemo } from "./demo";

/**
 * Nguồn cho hai bộ hoàn thành trong ô soạn tin: `@` ra tệp, `/` ra lệnh.
 *
 * Chấm điểm đường dẫn nằm ở **phía Rust** (`pai-index::complete`), không ở đây. Lý do là
 * dữ liệu: bảng `files` của một repo lớn có hàng chục nghìn hàng, và kéo cả bảng qua biên
 * IPC sau mỗi phím gõ là trả cái giá đắt nhất của Tauri ở đúng chỗ người dùng ít chịu
 * đựng nhất. Lõi lọc, giao diện vẽ.
 *
 * Lệnh `/` thì ngược lại — danh sách cố định, mười mấy mục, biết trước lúc biên dịch — nên
 * nó nằm hẳn ở đây và không tốn lời gọi nào.
 */

/** Một lệnh gõ được sau dấu `/`. */
export interface Command {
  name: string;
  /** Câu mô tả hiện cạnh tên trong danh sách. */
  hint: string;
  /**
   * Có cần một dự án đang mở không. Lệnh cần dự án vẫn **hiện** khi chưa mở, nhưng hiện
   * kèm lý do và không chọn được — giấu đi thì người dùng không bao giờ biết nó tồn tại,
   * và đó chính là vấn đề mà bảng lệnh sinh ra để sửa.
   */
  needsProject?: boolean;
}

/**
 * Từ vựng lệnh.
 *
 * Mỗi mục ở đây phải là một việc người dùng **đã làm được bằng cách khác** — bảng lệnh là
 * lối tắt, không phải một cửa sau dẫn tới khả năng chưa có giao diện nào khác chạm tới.
 * Một lệnh không có đường đi thứ hai là một lệnh không ai tìm lại được sau khi quên tên nó.
 */
export const COMMANDS: Command[] = [
  { name: "moi", hint: "Phiên mới" },
  { name: "tim", hint: "Tìm phiên đã có (⌘K)" },
  { name: "duan", hint: "Mở màn hình dự án" },
  { name: "thaydoi", hint: "Bảng thay đổi của lượt này", needsProject: true },
  { name: "taplieu", hint: "Thư viện tài liệu", needsProject: true },
  { name: "mohinh", hint: "Đổi nhà cung cấp và mô hình" },
  { name: "mcp", hint: "Quản lý server MCP" },
  { name: "quyen", hint: "Trang quyền và phạm vi tool" },
  { name: "phimtat", hint: "Danh sách phím tắt" },
  { name: "caidat", hint: "Cài đặt chung" },
];

/**
 * Lọc lệnh theo phần đã gõ sau dấu `/`.
 *
 * Ba bậc: khớp tiền tố tên, rồi khớp giữa tên, rồi khớp trong câu mô tả. Bậc cuối là bậc
 * đáng giá nhất — người ta nhớ **việc** mình muốn làm, không nhớ cái tên ta đặt cho nó, nên
 * gõ "phím tắt" phải tìm ra `phimtat`.
 *
 * Cùng bậc thì xếp theo bảng chữ cái, không theo thứ tự khai báo: thứ tự khai báo là thứ tự
 * ta nghĩ ra chúng, và nó đổi mỗi lần ai đó thêm một dòng.
 */
export function rankCommands(query: string): Command[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return COMMANDS;

  const scored: { command: Command; score: number }[] = [];
  for (const command of COMMANDS) {
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

/**
 * Đường dẫn khớp phần đã gõ sau dấu `@`.
 *
 * Nuốt lỗi và trả về rỗng: gợi ý là thứ **thêm vào**, và một hộp thoại lỗi bật ra giữa lúc
 * đang gõ là đổi một tiện ích lấy một gián đoạn. Danh sách rỗng đọc ra là "không có gì
 * khớp", và đó cũng là điều đúng khi lõi không trả lời được.
 */
export async function completePaths(query: string, limit = 8): Promise<string[]> {
  if (isDemo() || !inTauri()) return demoPaths(query, limit);
  try {
    return await invoke<string[]>("complete_paths", { query, limit });
  } catch (err) {
    console.error("không hoàn thành được đường dẫn", err);
    return [];
  }
}

/** Cái đang được gõ dở tại con trỏ, nếu nó là một lời gọi hoàn thành. */
export interface Trigger {
  kind: "path" | "command";
  /** Phần sau dấu dẫn, đã gõ tới con trỏ. */
  query: string;
  /** Vị trí dấu dẫn (`@` hoặc `/`) trong chuỗi. */
  start: number;
  /** Vị trí con trỏ, tức hết phần đã gõ. */
  end: number;
}

/**
 * Tìm lời gọi hoàn thành tại con trỏ, hoặc `null` khi không có.
 *
 * # Hai luật, và luật thứ hai mới là luật đáng nói
 *
 * `@` mở bộ hoàn thành tệp ở **bất kỳ đâu** miễn là nó đứng đầu một từ. Đứng giữa từ thì
 * không: `a@b` là một địa chỉ thư, không phải một lời gọi.
 *
 * `/` chỉ mở bộ lệnh khi nó là **ký tự đầu tiên của cả ô nhập**. Đây là chỗ dễ làm sai
 * nhất: nới ra thành "đầu một từ" thì mỗi lần gõ một đường dẫn — `src/lib`, `crates/pai-fs`
 * — là một lần bảng lệnh nhảy ra che mất chữ đang gõ, trong đúng một ứng dụng mà đường dẫn
 * là thứ người ta gõ suốt ngày.
 *
 * Có khoảng trắng trong phần đã gõ thì lời gọi kết thúc: người dùng đã đi qua nó rồi.
 */
export function findTrigger(text: string, caret: number): Trigger | null {
  const upto = text.slice(0, caret);

  // Lệnh: cả ô nhập phải bắt đầu bằng `/` và chưa có khoảng trắng nào.
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

/**
 * Thay phần đang gõ dở bằng giá trị đã chọn, và nói con trỏ đi đâu.
 *
 * Thêm một dấu cách ở cuối: gần như lần nào người dùng cũng gõ tiếp sau khi chèn một
 * đường dẫn, và bắt họ tự gõ dấu cách ấy là bắt họ trả một phím cho mỗi lần chèn.
 */
export function applyCompletion(
  text: string,
  trigger: Trigger,
  value: string,
): { text: string; caret: number } {
  const inserted = `${value} `;
  const next = text.slice(0, trigger.start) + inserted + text.slice(trigger.end);
  return { text: next, caret: trigger.start + inserted.length };
}
