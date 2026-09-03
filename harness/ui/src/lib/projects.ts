import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import type { CloneProgress, DirEntry, Project, ProjectKind } from "./protocol";

/**
 * Lệnh dự án và lệnh clone.
 *
 * Chia làm hai nhóm theo cách xử lý lỗi, và ranh giới là "người dùng có đang đứng chờ
 * một thứ hiện lên không":
 *
 *   - `listProjects` nuốt lỗi và trả danh sách rỗng — nó chạy lúc khởi động, và một hộp
 *     lỗi ở đó chặn mất đường vào ứng dụng.
 *   - mọi lệnh còn lại **ném ra ngoài**. Mở nhầm dự án, bỏ nhầm một dòng — cả hai đều là
 *     lúc người dùng vừa bấm một cú và đang chờ; im lặng ở đó không phân biệt được với
 *     "đang chậm".
 */

/**
 * Một tầng của cây thư mục dự án.
 *
 * **Một tầng, không đệ quy.** Bung một nhánh là một lời gọi; nhánh chưa bung thì chưa tốn
 * gì — nếu không thì mở bảng dự án là đọc cả `node_modules` cho một người chỉ định xem
 * thư mục `src`.
 *
 * Nuốt lỗi và trả rỗng: nó chạy khi người dùng bấm vào một hàng, và cách hỏng thường gặp
 * nhất là thư mục vừa bị xoá hay không có quyền đọc — cả hai đều đọc đúng thành "nhánh
 * này rỗng", chứ không đáng một hộp thoại chắn ngang.
 */
export async function listDir(path: string): Promise<DirEntry[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<DirEntry[]>("list_dir", { path });
  } catch (err) {
    console.error("không đọc được thư mục", path, err);
    return [];
  }
}

export async function listProjects(): Promise<Project[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<Project[]>("list_projects");
  } catch (err) {
    console.error("không đọc được danh sách dự án", err);
    return [];
  }
}

/** Mở một thư mục: thêm vào danh sách nếu mới, rồi chuyển sang nó. */
export function openProject(path: string): Promise<Project> {
  return invoke<Project>("open_project", { path });
}

/**
 * Bỏ một dự án **khỏi danh sách**. Thư mục trên đĩa không bị đụng tới.
 *
 * Tên hàm nói đúng điều lệnh làm, và mọi câu chữ đi kèm phải nói lại điều đó: một người
 * đọc "xoá dự án" trên màn hình sẽ hiểu là mất việc, và không có cách nào lấy lại niềm
 * tin đó sau khi họ đã không dám bấm.
 */
export function removeProject(id: string): Promise<void> {
  return invoke("remove_project", { id });
}

/**
 * Đóng dự án đang mở, quay về trò chuyện thuần tuý.
 *
 * Khác `removeProject` ở đúng chỗ quan trọng nhất: **danh sách không mất dòng nào**. Cái
 * bị tháo là nhánh plugin của tầng dự án — sau lệnh này trợ lý không còn tool nào chạm
 * tới đĩa, còn hội thoại chạy tiếp bình thường.
 *
 * Lõi trả lại cả danh sách sau khi đóng, nên chỗ gọi không phải hỏi thêm một vòng nữa mới
 * biết dòng nào còn đang sáng.
 *
 * Ném ra ngoài: nó chạy sau một cú bấm, và im lặng ở đó không phân biệt được với "đang chậm".
 */
export function closeProject(): Promise<Project[]> {
  return invoke<Project[]>("close_project");
}

/**
 * Đổi loại một dự án.
 *
 * Loại được đặt một lần lúc ghi nhận, và mở lại thì giữ nguyên — nên không có lệnh này
 * thì một thư mục vào nhầm loại là ngõ cụt: một repo lỡ ghi nhận thành thư viện tài liệu
 * sẽ mãi mãi không có `read`, `grep` hay `bash`, và người dùng chỉ thấy trợ lý nói nó
 * không có tool nào để đọc tệp.
 */
export function setProjectKind(id: string, kind: ProjectKind): Promise<Project[]> {
  return invoke<Project[]>("set_project_kind", { id, kind });
}

/** Tên hiển thị suy từ đường dẫn, dùng khi lõi chưa kịp trả `Project` thật. */
export function folderName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}




/**
 * Tạo một dự án từ một thư mục có sẵn, kèm **loại** của nó.
 *
 * Loại được người dùng chọn chứ không được lõi đoán, và đó là chủ ý: đoán "thư mục này
 * trông giống mã nguồn" rồi cắm tầng tool mã nguồn vào một thư mục toàn tệp người ngoài
 * gửi tới là cấp quyền chạy lệnh cho đúng chỗ không nên cấp. Một lần chọn sai của người
 * dùng thì sửa được; một lần đoán sai của máy thì không ai nhìn thấy để mà sửa.
 */
export function createProject(path: string, kind: ProjectKind): Promise<Project> {
  return invoke<Project>("create_project", { path, kind });
}

export interface CloneRequest {
  url: string;
  parent: string;
  /** Tên thư mục đích. Vắng thì lõi suy từ URL. */
  name?: string;
  /** `1` = chỉ lấy lịch sử gần nhất. Vắng nghĩa là clone đầy đủ. */
  depth?: number;
  /**
   * Loại dự án sau khi clone xong. Mặc định `"code"`.
   *
   * Có mặt vì lõi bắt buộc phải nhận một loại, và bỏ trống để lõi đoán là đúng cái bẫy
   * `createProject` đã tránh. Mặc định `"code"` vì lối vào duy nhất dẫn tới đây là nút
   * "Clone từ Git" ở nhóm mã nguồn — không phải một suy đoán, mà là chỗ người dùng bấm.
   */
  kind?: ProjectKind;
}

/**
 * Clone một repo về rồi mở nó làm dự án.
 *
 * Tiến trình đi qua `Channel` chứ không qua `listen`, cùng lý do với `sendMessage`:
 * channel gắn với đúng một lần clone và tự dọn khi bị bỏ, nên một hộp thoại đã đóng
 * không còn listener nào sống sót để vẽ tiếp lên màn hình đã biến mất.
 *
 * Ném ra ngoài. Clone hỏng là chuyện thường — URL sai, không có mạng, thư mục đã tồn tại
 * — và mỗi lý do đó cần đến được mắt người dùng nguyên văn, vì chỉ git mới biết nó là gì.
 */
export function cloneProject(
  req: CloneRequest,
  onProgress: (p: CloneProgress) => void,
): Promise<Project> {
  const channel = new Channel<CloneProgress>();
  channel.onmessage = onProgress;
  // Trải phẳng chứ không gói trong `req`: lõi nhận từng tham số một, và một object lồng
  // sẽ tới nơi dưới dạng một tham số không ai đọc — lỗi chỉ lộ ra lúc chạy.
  return invoke<Project>("clone_project", {
    url: req.url,
    parent: req.parent,
    name: req.name,
    depth: req.depth,
    kind: req.kind ?? "code",
    onProgress: channel,
  });
}

/**
 * Huỷ lần clone đang chạy.
 *
 * Nuốt lỗi, giống `cancelTurn`: người dùng đã quyết định dừng rồi, và một hộp lỗi vì
 * "không huỷ được" chỉ thêm một thứ nữa phải đóng. Lời cuối vẫn thuộc về `cloneProject`,
 * thứ sẽ trả về hoặc ném ngay sau đó.
 */
export async function cancelClone(): Promise<void> {
  if (!inTauri()) return;
  try {
    await invoke("cancel_clone");
  } catch (err) {
    console.error("không huỷ được lần clone", err);
  }
}

/**
 * Hộp thoại chọn thư mục của hệ điều hành. `null` = người dùng bấm huỷ.
 *
 * Ngoài Tauri thì trả `null` chứ không ném: `npm run dev` trong trình duyệt không có hộp
 * thoại nào, và ở đó lối gõ tay vẫn phải mở được dự án. Lỗi thật của plugin thì để nó
 * ném — một hộp thoại hệ thống bấm vào rồi không có gì xảy ra là thứ không ai gỡ được.
 */
export async function pickDirectory(title?: string): Promise<string | null> {
  if (!inTauri()) return null;
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

/**
 * Host của một URL nguồn gốc, để làm huy hiệu.
 *
 * Huy hiệu chỉ có chỗ cho vài ký tự, và phần phân biệt được giữa hai URL clone là *máy
 * chủ* chứ không phải đường dẫn — hai repo cùng tên trên GitHub và trên một GitLab nội
 * bộ là hai thứ khác nhau. Dạng scp (`git@host:user/repo.git`) không phải URL hợp lệ với
 * `new URL`, nên phải bắt riêng thay vì để nó rơi xuống nhánh trả nguyên chuỗi.
 */
export function originHost(origin: string): string {
  const scp = /^[^@/]+@([^:/]+):/.exec(origin);
  if (scp?.[1] !== undefined) return scp[1];
  try {
    return new URL(origin).host || origin;
  } catch {
    return origin;
  }
}

/** Tên thư mục suy từ URL repo, dùng làm gợi ý mặc định trong hộp thoại clone. */
export function repoNameFromUrl(url: string): string {
  const cleaned = url.trim().replace(/\/+$/, "").replace(/\.git$/i, "");
  const tail = cleaned.split(/[/:]/).pop() ?? "";
  return tail;
}
