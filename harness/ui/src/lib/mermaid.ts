import { createSignal } from "solid-js";
import type { MermaidConfig } from "mermaid";
import { theme } from "./theme";

/**
 * Tầng bọc quanh mermaid. Ba quyết định ở đây không phải chuyện thẩm mỹ.
 *
 * **Nạp trễ.** Gói mermaid nặng cỡ một megabyte sau khi nén. Phần lớn phiên làm việc
 * không có sơ đồ nào, và nạp sẵn nghĩa là mọi người dùng trả cái giá đó lúc khởi động
 * để đổi lấy một thứ họ không dùng. `import()` chỉ chạy ở lần vẽ đầu tiên.
 *
 * **`securityLevel: "strict"` và `htmlLabels: false`.** Nguồn sơ đồ do mô hình sinh ra,
 * mà mô hình vừa đọc tài liệu người dùng nạp lên — nên chuỗi này thực chất có thể do
 * một người ngoài viết. Nhãn dựng bằng HTML là một đường tiêm HTML thẳng vào cửa sổ ứng
 * dụng, và cửa sổ này có `invoke` của Tauri trong tầm với. Nhãn dựng bằng `<text>` của
 * SVG thì không có đường đó. Đây là hàng rào, không phải tuỳ chọn hiển thị.
 *
 * **Một hàng đợi.** `mermaid.render` giữ trạng thái toàn cục theo id và chèn một nút tạm
 * vào `document.body`; hai lần gọi chồng lên nhau thì cái sau ăn mất khung của cái
 * trước. Một bản ghi hội thoại có ba bốn sơ đồ là chuyện bình thường ở đây, nên nối tiếp
 * là bắt buộc chứ không phải phòng xa.
 */

export type DiagramRender = { ok: true; svg: string } | { ok: false; message: string };

type MermaidModule = typeof import("mermaid").default;

let pending: Promise<MermaidModule> | null = null;

/** Chỉ nạp một lần cho cả phiên; lỗi mạng thì cho phép thử lại ở lần vẽ sau. */
function load(): Promise<MermaidModule> {
  if (pending === null) {
    pending = import("mermaid")
      .then((mod) => mod.default)
      .catch((err) => {
        pending = null;
        throw err;
      });
  }
  return pending;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function prefersDark(): boolean {
  try {
    return window.matchMedia(DARK_QUERY).matches;
  } catch {
    return false;
  }
}

const [systemDark, setSystemDark] = createSignal(prefersDark());

try {
  window.matchMedia(DARK_QUERY).addEventListener("change", (event) => setSystemDark(event.matches));
} catch {
  /* môi trường không có matchMedia thì coi như sáng — chỉ ảnh hưởng màu, không ảnh hưởng chạy */
}

/**
 * Đang ở chế độ tối hay không.
 *
 * `theme()` một mình không đủ: lựa chọn "system" không stamp gì lên `<html>` (xem
 * theme.ts), nên phải hỏi thêm media query. Đây là một signal, và chỗ vẽ sơ đồ đọc nó
 * để vẽ lại — mermaid nướng màu thẳng vào SVG nên đổi theme mà không vẽ lại thì sơ đồ
 * giữ nguyên bảng màu cũ giữa một trang đã đổi màu.
 */
export function isDark(): boolean {
  const choice = theme();
  return choice === "dark" || (choice === "system" && systemDark());
}

function palette(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  // Chỉ đọc token có giá trị nguyên thuỷ. Token dựng bằng `color-mix` (--overlay-*,
  // --glass) không được thay thế ở computed value, nên đọc ra là chuỗi hàm mermaid
  // không hiểu.
  const read = (name: string, fallback: string): string => {
    const value = style.getPropertyValue(name).trim();
    return value === "" ? fallback : value;
  };
  return {
    bg: read("--bg", "#f3f6f4"),
    surface: read("--surface", "#ffffff"),
    surfaceSoft: read("--surface-soft", "#f7f9f8"),
    ink: read("--ink", "#17231f"),
    text: read("--text", "#293732"),
    muted: read("--muted", "#55635d"),
    line: read("--line", "#d8e0dc"),
    lineStrong: read("--line-strong", "#87968f"),
    accent: read("--accent", "#176b59"),
    accentSoft: read("--accent-soft", "#deeee8"),
    accentInk: read("--accent-ink", "#0c4d3f"),
    warnSoft: read("--warn-soft", "#f6ecd8"),
    warn: read("--warn", "#8a5a12"),
    dangerSoft: read("--danger-soft", "#f8e5e3"),
    danger: read("--danger", "#ad403c"),
    font: read("--font-ui", "sans-serif"),
  };
}

/**
 * Cấu hình dựng lại trước **mỗi** lần vẽ.
 *
 * Bảng màu lấy từ token của repo chứ không dùng bộ mặc định của mermaid: một sơ đồ màu
 * tím nhạt nằm giữa một giao diện xanh rêu trông như ảnh dán từ trang khác vào, và người
 * đọc mất một nhịp để hiểu nó thuộc về đây.
 */
function config(): MermaidConfig {
  const c = palette();
  return {
    startOnLoad: false,
    securityLevel: "strict",
    htmlLabels: false,
    // Mermaid vẽ sẵn một khung đỏ vào DOM khi cú pháp hỏng. Ta tự lo phần đó để còn kèm
    // được mã nguồn bên cạnh thông điệp — nên tắt.
    suppressErrorRendering: true,
    theme: "base",
    fontFamily: c.font,
    flowchart: { htmlLabels: false, useMaxWidth: true, curve: "basis", padding: 12 },
    sequence: { useMaxWidth: true, wrap: true },
    class: { htmlLabels: false, useMaxWidth: true },
    state: { useMaxWidth: true },
    er: { useMaxWidth: true },
    themeVariables: {
      darkMode: isDark(),
      background: c.surface,
      fontFamily: c.font,
      fontSize: "13px",
      primaryColor: c.accentSoft,
      primaryTextColor: c.accentInk,
      primaryBorderColor: c.accent,
      secondaryColor: c.surfaceSoft,
      secondaryTextColor: c.text,
      secondaryBorderColor: c.line,
      tertiaryColor: c.bg,
      tertiaryTextColor: c.text,
      tertiaryBorderColor: c.line,
      mainBkg: c.surfaceSoft,
      nodeBorder: c.lineStrong,
      nodeTextColor: c.ink,
      titleColor: c.ink,
      textColor: c.text,
      lineColor: c.lineStrong,
      edgeLabelBackground: c.surface,
      clusterBkg: c.bg,
      clusterBorder: c.line,
      labelBackground: c.surface,
      noteBkgColor: c.warnSoft,
      noteTextColor: c.warn,
      noteBorderColor: c.warn,
      errorBkgColor: c.dangerSoft,
      errorTextColor: c.danger,
    },
  };
}

let queue: Promise<unknown> = Promise.resolve();
let seq = 0;

function reason(err: unknown): string {
  if (err instanceof Error && err.message !== "") return err.message;
  if (typeof err === "string" && err !== "") return err;
  // `DetailedError` của mermaid không phải Error thật; nó mang `str` là dòng hỏng.
  if (err !== null && typeof err === "object" && "str" in err) return String(err.str);
  return "Mermaid không đọc được sơ đồ này.";
}

/**
 * Vẽ một sơ đồ. Không bao giờ ném — cú pháp hỏng là **kết quả**, không phải sự cố.
 *
 * Lý do: mô hình sinh sai cú pháp mermaid thường xuyên, và ở chỗ gọi thì "vẽ hỏng" cần
 * hiện ra cho người dùng đọc chứ không cần một `try` nữa. Gọi `parse` trước `render` vì
 * `render` thất bại nửa chừng vẫn để lại nút tạm trong `body`.
 */
export function renderDiagram(source: string): Promise<DiagramRender> {
  const job = async (): Promise<DiagramRender> => {
    let mermaid: MermaidModule;
    try {
      mermaid = await load();
    } catch (err) {
      console.error("không nạp được mermaid", err);
      return { ok: false, message: "Không nạp được bộ vẽ sơ đồ." };
    }

    const id = `pai-mermaid-${(seq += 1)}`;
    try {
      mermaid.initialize(config());
      await mermaid.parse(source);
      const { svg } = await mermaid.render(id, source);
      return { ok: true, svg };
    } catch (err) {
      return { ok: false, message: reason(err) };
    } finally {
      // Nút tạm mermaid chèn vào `body`. Nó tự dọn khi vẽ xong, nhưng không dọn khi vẽ
      // hỏng giữa chừng — và mỗi cái để lại là một khối chiếm chỗ vô hình trong trang.
      document.getElementById(id)?.remove();
      document.getElementById(`d${id}`)?.remove();
    }
  };

  const next = queue.then(job, job);
  queue = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

const KIND_LABEL: Record<string, string> = {
  flowchart: "lưu đồ",
  graph: "lưu đồ",
  sequencediagram: "sơ đồ tuần tự",
  classdiagram: "sơ đồ lớp",
  statediagram: "sơ đồ trạng thái",
  erdiagram: "sơ đồ thực thể",
  journey: "hành trình người dùng",
  gantt: "biểu đồ gantt",
  pie: "biểu đồ tròn",
  mindmap: "sơ đồ tư duy",
  timeline: "dòng thời gian",
  gitgraph: "đồ thị git",
  quadrantchart: "biểu đồ bốn góc",
  requirementdiagram: "sơ đồ yêu cầu",
  block: "sơ đồ khối",
  sankey: "sơ đồ dòng chảy",
  xychart: "biểu đồ toạ độ",
  architecture: "sơ đồ kiến trúc",
  packet: "sơ đồ gói tin",
  c4context: "sơ đồ C4",
};

/**
 * Loại sơ đồ, đọc từ dòng khai báo đầu tiên.
 *
 * Dùng cho `aria-label`: SVG mermaid sinh ra gần như không đọc được bằng trình đọc màn
 * hình, nên ít nhất phải nói được đây là *loại* hình gì trước khi người dùng chuyển sang
 * xem mã nguồn.
 */
export function diagramKind(source: string): string {
  let body = source.trimStart();
  // Khối frontmatter `--- ... ---` đứng trước dòng khai báo; bỏ qua nó.
  if (body.startsWith("---")) {
    const end = body.indexOf("\n---", 3);
    if (end !== -1) body = body.slice(end + 4);
  }
  for (const raw of body.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("%%")) continue;
    const token = /^([A-Za-z0-9]+)/.exec(line)?.[1];
    if (token === undefined) break;
    return KIND_LABEL[token.toLowerCase()] ?? "sơ đồ";
  }
  return "sơ đồ";
}
