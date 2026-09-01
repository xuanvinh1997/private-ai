import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import type { FileView, Project, TreeEntry } from "./protocol";

/**
 * Bốn lệnh dự án và hai lệnh duyệt mã nguồn.
 *
 * Chia làm hai nhóm theo cách xử lý lỗi, và ranh giới là "người dùng có đang đứng chờ
 * một thứ hiện lên không":
 *
 *   - `listProjects` nuốt lỗi và trả danh sách rỗng — nó chạy lúc khởi động, và một hộp
 *     lỗi ở đó chặn mất đường vào ứng dụng.
 *   - mọi lệnh còn lại **ném ra ngoài**. Mở nhầm dự án, bỏ nhầm một dòng, mở một tệp
 *     không đọc được — cả ba đều là lúc người dùng vừa bấm một cú và đang chờ; im lặng
 *     ở đó không phân biệt được với "đang chậm".
 */

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
 * Một cấp của cây tệp. `path` vắng nghĩa là gốc dự án đang mở.
 *
 * `depth` mặc định là 1 và nên giữ nguyên như vậy cho mọi lần mở một thư mục: một repo
 * thật có hàng chục nghìn tệp, và nạp cả cây một lần là treo giao diện trước khi vẽ được
 * dòng đầu tiên. Chỗ duy nhất xin sâu hơn là bảng tìm tệp, nơi *phải* có toàn bộ tên.
 */
export function listTree(path?: string, depth = 1): Promise<TreeEntry[]> {
  return invoke<TreeEntry[]>("list_tree", { path, depth });
}

export function readFile(path: string): Promise<FileView> {
  return invoke<FileView>("read_file", { path });
}

/* Không có `pickDirectory()` trong tệp này, và đó là một khoảng trống có chủ đích: hộp
 * thoại chọn thư mục của hệ điều hành cần `@tauri-apps/plugin-dialog`, thứ chưa nằm
 * trong `package.json`. Thêm nó từ phía giao diện là sửa cả `Cargo.toml` lẫn danh sách
 * quyền của Tauri — việc của phía Rust. Cho tới lúc đó `OpenProjectDialog` nhận đường
 * dẫn bằng tay, và kéo thả một thư mục vào cửa sổ là lối không phải gõ. */

/** Tên hiển thị suy từ đường dẫn, dùng khi lõi chưa kịp trả `Project` thật. */
export function folderName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

const isAbsolute = (path: string): boolean =>
  path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path);

/**
 * Đường dẫn tuyệt đối, dựng từ gốc dự án khi chỗ gọi chỉ có đường dẫn tương đối.
 *
 * Hai nguồn đường dẫn trong ứng dụng nói hai thứ tiếng khác nhau, và đó không phải lỗi
 * của bên nào: `list_tree` trả đường dẫn tuyệt đối vì nó phải chứng minh được tệp nằm
 * trong dự án, còn `ToolMeta` mang đường dẫn tương đối vì đó là thứ mô hình đọc được.
 * `read_file` chuẩn hoá đường dẫn nó nhận, nên một đường dẫn tương đối ở đó sẽ được giải
 * theo thư mục làm việc của tiến trình — im lặng trỏ ra ngoài dự án. Quy về một mối ở
 * đây, đúng một lần, tại chỗ mở tệp.
 */
export function absolutePath(root: string | null, path: string): string {
  if (root === null || isAbsolute(path)) return path;
  return `${root.replace(/[/\\]+$/, "")}/${path}`;
}

/** Bỏ tiền tố gốc dự án khi hiện cho người đọc. Gốc dự án là thứ ai cũng đã biết. */
export function displayPath(root: string | null, path: string): string {
  if (root === null) return path;
  const base = `${root.replace(/[/\\]+$/, "")}/`;
  return path.startsWith(base) ? path.slice(base.length) : path;
}
