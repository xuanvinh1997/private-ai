import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import { isDemo } from "./demo";

/**
 * Đính kèm tệp vào tin nhắn.
 *
 * Hai lối vào, một đường đi: hộp thoại của hệ điều hành và kéo thả đều cho ra **đường dẫn
 * tuyệt đối**, rồi cả hai đi qua `resolveAttachments` trước khi chạm vào bản nháp. Cái mà
 * HTML5 `<input type="file">` không cho là đường dẫn, chứ không phải là hộp thoại — nên
 * "chỉ kéo thả mới đính kèm được" là một luật sai, và ô soạn tin từng là màn hình duy nhất
 * còn tin vào nó trong khi thư viện tài liệu đã dùng hộp thoại này từ đầu (`lib/docs.ts`).
 */

/** Bản sao TypeScript của `Attachment` trong `app/src/commands/attach.rs`. */
export interface Attachment {
  path: string;
  error: string | null;
}

/**
 * Hộp thoại chọn tệp của hệ điều hành.
 *
 * Không lọc theo đuôi tệp: dự án chứa gì thì đính kèm được cái đó, và một danh sách đuôi
 * chép sang giao diện sẽ lệch đúng vào lúc lõi đọc thêm được một loại — người dùng gặp một
 * hộp thoại từ chối chính cái tệp mà trợ lý đọc được. Cùng lý do với `pickDocuments`.
 *
 * Ba kết quả, không phải hai, và cái thứ ba mới là cái hay bị nuốt: `[]` là **người dùng
 * bấm Huỷ** — im lặng là đúng, họ vừa tự tay nói không. `null` là **không có hộp thoại nào
 * ở đây**, tức đang chạy trong trình duyệt chứ không trong ứng dụng. Gộp hai thứ ấy vào
 * cùng một `[]` là biến một nút hỏng thành một nút trông như vừa bị huỷ, và người bấm nó
 * không có cách nào phân biệt "tôi vừa huỷ" với "cái nút này chết".
 */
export async function pickFiles(): Promise<string[] | null> {
  if (!inTauri()) return null;
  const picked = await open({ directory: false, multiple: true, title: "Đính kèm tệp" });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/**
 * Hỏi lõi xem những đường dẫn này có đính kèm được không.
 *
 * Ngoài Tauri thì nhận hết: trang demo không có đĩa để hỏi, và một danh sách toàn lỗi ở đó
 * là dựng sai trạng thái chứ không phải dựng được trạng thái lỗi.
 *
 * **Ném ra ngoài** thay vì nuốt, ngược với `completePaths`: người dùng vừa buông chuột và
 * đang chờ thấy đường dẫn hiện ra trong ô soạn tin. Im lặng ở đây đọc ra là "cú thả không
 * ăn", và họ sẽ thả lại.
 */
export async function resolveAttachments(paths: string[]): Promise<Attachment[]> {
  if (isDemo() || !inTauri()) return paths.map((path) => ({ path, error: null }));
  return await invoke<Attachment[]>("resolve_attachments", { paths });
}
