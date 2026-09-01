import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AgentEvent,
  ApprovalDecision,
  HistoryNode,
  ModelChoice,
  SessionSummary,
} from "./protocol";

/**
 * Ứng dụng có chạy trong vỏ Tauri không.
 *
 * `npm run dev` mở trong trình duyệt thường xuyên hơn mở trong cửa sổ Tauri, và mọi
 * lệnh `invoke` ở đó đều ném. Chặn ở một chỗ để phần còn lại của giao diện không phải
 * rải try/catch — và để trang demo chạy được mà không cần backend.
 */
export const inTauri = (): boolean => {
  try {
    return isTauri();
  } catch {
    return false;
  }
};

/**
 * Gửi một lượt.
 *
 * Dùng `Channel` chứ không `listen`: channel gắn với đúng một lượt, giữ thứ tự, và tự
 * dọn khi bị bỏ — nên hai lượt chạy song song không trộn token vào nhau, và không có
 * listener nào sống sót qua lượt đã kết thúc.
 *
 * Việc gộp token (coalescing) nằm ở phía Rust: mỗi lần vượt biên IPC của Tauri đắt hơn
 * nhiều so với một signal của Qt, nên phát từng token một sẽ nghẽn khi mô hình chạy nhanh.
 */
export function sendMessage(
  sessionId: string,
  text: string,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("send_message", { input: { sessionId, text }, onEvent: channel });
}

/**
 * Trả lời một yêu cầu duyệt.
 *
 * Fail-closed: nếu vì bất kỳ lý do gì lệnh này không tới được lõi, lõi phải coi như
 * *từ chối*. Đó là lý do hàm nuốt lỗi thay vì ném — chỗ gọi đã quyết định rồi, và một
 * exception ở đây chỉ làm hộp thoại kẹt mở trong khi lượt phía dưới đã bị chặn.
 */
export async function answerApproval(
  requestId: string,
  decision: ApprovalDecision,
): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("approval_result", { requestId, decision });
  } catch (err) {
    console.error("không gửi được quyết định duyệt", err);
  }
}

/** Huỷ lượt đang chạy. Không có lõi thì im lặng — nút vẫn phải bấm được trong demo. */
export async function cancelTurn(sessionId: string): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("cancel_turn", { sessionId });
  } catch (err) {
    console.error("không huỷ được lượt", err);
  }
}

export async function listSessions(): Promise<SessionSummary[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<SessionSummary[]>("list_sessions");
  } catch (err) {
    console.error("không đọc được danh sách phiên", err);
    return [];
  }
}

export async function createSession(title: string): Promise<SessionSummary | null> {
  if (!inTauri()) return null;
  try {
    return await invoke<SessionSummary>("create_session", { title });
  } catch (err) {
    console.error("không tạo được phiên", err);
    return null;
  }
}

/**
 * Đổi tên một phiên.
 *
 * Nuốt lỗi và ghi log: tên hiển thị đã đổi trên màn hình rồi, và một hộp thoại lỗi vì
 * không ghi được cái tên chỉ làm gián đoạn việc người dùng đang làm.
 */
export async function renameSession(sessionId: string, title: string): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("rename_session", { sessionId, title });
  } catch (err) {
    console.error("không đổi được tên phiên", err);
  }
}

/**
 * Xoá một phiên.
 *
 * **Ném lỗi ra ngoài**, khác với `renameSession`: xoá là hành động không hoàn lại được,
 * nên "tưởng đã xoá mà chưa" là một trạng thái người dùng phải biết. Lõi cũng trả lỗi có
 * tên khi phiên không tồn tại, nên bấm Xoá hai lần thì lần sau hiện lỗi thay vì im lặng.
 */
export async function deleteSession(sessionId: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("delete_session", { sessionId });
}

/**
 * Bản ghi đã lưu của một phiên.
 *
 * Không có lệnh này thì bấm vào một phiên cũ chỉ ra màn hình trống: danh sách phiên
 * trông có việc nhưng không dẫn tới đâu. Ném lỗi ra ngoài vì đây đúng là chỗ người dùng
 * đang chờ một thứ hiện lên — im lặng ở đây không phân biệt được với "phiên này rỗng".
 */
export async function loadSession(sessionId: string): Promise<HistoryNode[]> {
  if (!inTauri()) return [];
  return await invoke<HistoryNode[]>("load_session", { sessionId });
}

/**
 * Mô hình máy chủ đang có.
 *
 * Danh sách rỗng nghĩa là **máy chủ không trả lời được**, không phải "không có mô hình
 * nào" — lõi đã nuốt lỗi mạng ở phía nó rồi. Người gọi phải nói đúng điều đó.
 */
export async function listModels(): Promise<ModelChoice[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<ModelChoice[]>("list_models");
  } catch (err) {
    console.error("không đọc được danh sách mô hình", err);
    return [];
  }
}
