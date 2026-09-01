import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "../../lib/agent";

/**
 * Ba lệnh lõi cho màn hình cài đặt.
 *
 * Nằm trong `settings/` chứ không trong `lib/` vì đúng hai trang cài đặt dùng tới nó và
 * không có đường nào khác trong ứng dụng gọi vào. Ngày nào lõi có thêm lệnh nói về vòng
 * giam hay về hook thì cả ba dọn chung lên `lib/harness.ts`.
 *
 * `describe_harness` trả về **bản in cho con người** của cây plugin — đọc được *hàng nào
 * đang cắm và cấu hình đến từ tệp nào*, không đọc được nội dung cấu hình. `sandbox_status`
 * và `list_hooks` bù đúng hai chỗ trống ấy: mức giam thật, và hook thật.
 */

/** Mỗi phần tử là một dòng. Ném ra ngoài khi lõi không dựng được cây — xem `createResource`. */
export async function describeHarness(): Promise<string[]> {
  // Ngoài Tauri (trang demo, `npm run dev`) thì không có lõi nào để hỏi. Trả rỗng chứ
  // không ném: "không có lõi" là một trạng thái bình thường của trang demo, còn "lõi trả
  // lỗi" là một sự cố, và hai thứ đó phải hiện ra khác nhau.
  if (!inTauri()) return [];
  return await invoke<string[]>("describe_harness");
}

/** Một hàng trong cây plugin: `id`, tên plugin, và dấu vết những lớp cấu hình đã đụng vào nó. */
export interface HarnessRow {
  id: string;
  plugin: string;
  /** `nen (dựng sẵn)`, hoặc `nen (dựng sẵn) → /Users/…/.private-ai/patch.yaml`. */
  origin: string;
  disabled: boolean;
}

/**
 * Đọc bản in của lõi thành từng hàng.
 *
 * Bản in có dạng hai dòng một hàng: `id: plugin` rồi `  # lớp → lớp`. Phân tích nó ở đây
 * là chấp nhận một ràng buộc thật — đổi định dạng in bên Rust sẽ làm hàm này im lặng trả
 * về rỗng. Đó là cái giá của việc dùng một lệnh chẩn đoán sẵn có thay vì chờ một lệnh
 * mới; chỗ hỏng thì hiện ra là "chưa đọc được", không phải một câu trả lời sai.
 */
export function docCayPlugin(lines: string[]): HarnessRow[] {
  const rows: HarnessRow[] = [];
  for (let at = 0; at < lines.length; at += 1) {
    const head = lines[at] ?? "";
    // Dòng bắt đầu bằng khoảng trắng là dòng dấu vết, không phải dòng hàng. Dòng rỗng và
    // phần "# service đang cắm" ở cuối cũng rơi ra ngoài nhờ điều kiện này.
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

/** Một lớp cấu hình của người dùng đã đụng vào hàng này chưa. */
export function daVa(row: HarnessRow | undefined): boolean {
  return row !== undefined && row.origin.includes("→");
}


/** Vòng giam tiến trình, như lõi báo cáo nó. */
export interface SandboxStatus {
  /** `full` · `partial` · `none`. */
  mode: string;
  /** Vì sao chỉ thủng, hoặc vì sao không có gì. `null` khi `full`. */
  reason: string | null;
  writableRoots: string[];
  platform: string;
}

/**
 * Mức giam thật.
 *
 * `null` khi không hỏi được — và màn hình phải nói ra điều đó thay vì hiện `none`. Hai câu
 * ấy khác nhau hoàn toàn: một câu nói lệnh không trả lời, câu kia nói máy này **không có**
 * vòng giam nào, và câu thứ hai là một khẳng định về an toàn.
 */
export async function sandboxStatus(): Promise<SandboxStatus | null> {
  if (!inTauri()) return null;
  try {
    return await invoke<SandboxStatus>("sandbox_status");
  } catch (err) {
    console.error("không hỏi được mức giam", err);
    return null;
  }
}

export interface HookRow {
  command: string;
  /** Rỗng = áp cho mọi tool. */
  tools: string[];
  timeoutSecs: number | null;
  /** Lớp cấu hình đã khai nó. */
  origin: string;
}

/** Hook đang cài. Danh sách rỗng nghĩa là chưa cài hook nào — đó là mặc định. */
export async function listHooks(): Promise<HookRow[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<HookRow[]>("list_hooks");
  } catch (err) {
    console.error("không đọc được danh sách hook", err);
    return [];
  }
}

/** Đường dẫn tệp vá thật, tôn trọng `PAI_DATA_DIR`. Rỗng khi chạy ngoài Tauri. */
export async function hookConfigPath(): Promise<string> {
  if (!inTauri()) return "~/.private-ai/patch.yaml";
  try {
    return await invoke<string>("hook_config_path");
  } catch {
    return "~/.private-ai/patch.yaml";
  }
}
