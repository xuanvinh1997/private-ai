import type { CloneProgress, Project, ProjectKind } from "../protocol";

/**
 * Dữ liệu mẫu cho màn hình dự án ở chế độ `?demo=1`.
 *
 * Bộ mẫu được chọn theo **trạng thái cần nhìn thấy**, không theo "một danh sách trông
 * hợp lý": một dự án đang mở (không bỏ được), một dự án clone về (có huy hiệu nguồn
 * gốc), một thư viện tài liệu, và một dự án mã nguồn cũ để bộ lọc theo loại có việc để
 * làm. Một trạng thái không có trong dữ liệu mẫu là một trạng thái chưa ai nhìn thấy bao
 * giờ — kể cả người viết ra nó.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function demoProjectList(now = Date.now()): Project[] {
  return [
    {
      id: "p-harness",
      name: "harness",
      path: "/Users/vinhpx/Workspaces/private-ai/harness",
      lastOpenedAt: now - 3 * MINUTE,
      isCurrent: true,
      kind: "code",
      origin: null,
    },
    {
      id: "p-python",
      name: "private-ai",
      path: "/Users/vinhpx/Workspaces/private-ai",
      lastOpenedAt: now - 22 * HOUR,
      isCurrent: false,
      kind: "code",
      origin: "https://github.com/vinhpx/private-ai.git",
    },
    {
      id: "p-notes",
      name: "so-tay",
      path: "/Users/vinhpx/Documents/so-tay",
      lastOpenedAt: now - 6 * DAY,
      isCurrent: false,
      kind: "docs",
      origin: null,
    },
    {
      // Clone qua SSH: huy hiệu nguồn gốc phải rút được host từ dạng scp, thứ `new URL`
      // không đọc nổi. Không có mục này thì nhánh đó không bao giờ chạy trong lúc xem.
      id: "p-quy-dinh",
      name: "quy-dinh-noi-bo",
      path: "/Users/vinhpx/Documents/quy-dinh-noi-bo",
      lastOpenedAt: now - 19 * DAY,
      isCurrent: false,
      kind: "docs",
      origin: "git@gitlab.noi-bo.vn:phap-che/quy-dinh.git",
    },
  ];
}

/** Dự án lõi sẽ trả về sau khi tạo — dùng để hộp thoại trong demo có thứ để trả. */
export function demoCreatedProject(path: string, kind: ProjectKind): Project {
  const name = path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;
  return {
    id: `demo-${path}`,
    name,
    path,
    lastOpenedAt: Date.now(),
    isCurrent: true,
    kind,
    origin: null,
  };
}

/**
 * Một lần clone giả, kể lại đúng hình dạng khó của tiến trình thật.
 *
 * Hai pha đầu **không có `percent`** — `git` không đếm được ở đó, và đúng chỗ đó là nơi
 * một thanh tiến trình đứng im ở 0% trông giống hệt một tiến trình đã treo. Bộ khung này
 * tồn tại để nhìn thấy cái khoảnh khắc ấy mà không cần mạng chậm.
 */
export function demoCloneFrames(url: string, path: string): CloneProgress[] {
  const step = (
    phase: string,
    percent: number | null,
    line: string | null,
  ): CloneProgress => ({ phase, percent, line, finished: false, path: null, error: null });

  return [
    step("Đang kết nối", null, `Cloning into '${path}'...`),
    step("Đang đếm đối tượng", null, "remote: Enumerating objects: 4821, done."),
    step("Đang nhận đối tượng", 12, "Receiving objects:  12% (579/4821)"),
    step("Đang nhận đối tượng", 48, "Receiving objects:  48% (2314/4821)"),
    step("Đang nhận đối tượng", 91, "Receiving objects:  91% (4387/4821)"),
    step("Đang giải nén", 100, "Resolving deltas: 100% (2140/2140), done."),
    { phase: "Xong", percent: 100, line: `Đã clone ${url}`, finished: true, path, error: null },
  ];
}
