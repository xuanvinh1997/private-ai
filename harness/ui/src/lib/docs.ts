import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { inTauri } from "./agent";
import type {
  DocumentFormat,
  DocumentHit,
  DocumentView,
  IngestProgress,
  LibraryStats,
} from "./protocol";

/**
 * Năm lệnh của thư viện tài liệu.
 *
 * Chia lỗi theo đúng ranh giới của `projects.ts`, và ranh giới đó là "người dùng có vừa
 * bấm một cú và đang đứng chờ không":
 *
 *   - `listDocuments` và `libraryStats` chạy lúc mở màn hình, nên **nuốt lỗi** và trả về
 *     một giá trị rỗng nhưng nói thật. Một hộp lỗi ở đó chặn mất cả màn hình vì một con
 *     số thống kê.
 *   - `addDocuments`, `removeDocument`, `searchDocuments` **ném ra ngoài**. Cả ba đều đi
 *     sau một cú bấm, và im lặng sau một cú bấm không phân biệt được với "đang chậm".
 */

export async function listDocuments(): Promise<DocumentView[]> {
  if (!inTauri()) return [];
  try {
    return await invoke<DocumentView[]>("list_documents");
  } catch (err) {
    console.error("không đọc được danh sách tài liệu", err);
    return [];
  }
}

/**
 * Sức khoẻ thư viện.
 *
 * Khi không hỏi được, trả về `semanticReady: false` kèm `reason` nói đúng chuyện đã xảy
 * ra — chứ không phải một bản thống kê rỗng trông như một thư viện trống. Hai thứ đó dẫn
 * người dùng đi hai đường khác nhau: một bên là nạp thêm tài liệu, một bên là mở lại ứng
 * dụng.
 */
export async function libraryStats(): Promise<LibraryStats> {
  const unknown: LibraryStats = {
    documents: 0,
    chunks: 0,
    embeddedChunks: 0,
    embedder: null,
    semanticReady: false,
    reason: "Không hỏi được tình trạng thư viện.",
    root: "",
    filesSeen: 0,
    filesSkipped: 0,
    unreadable: 0,
    excluded: 0,
    // `null` chứ không phải `0`: chưa hỏi được thì cũng chưa biết đã quét lần nào chưa,
    // và `0` ở đây sẽ hiện thành "quét lúc 1/1/1970".
    scannedAt: null,
    scanning: null,
  };
  if (!inTauri()) return unknown;
  try {
    return await invoke<LibraryStats>("library_stats");
  } catch (err) {
    console.error("không đọc được tình trạng thư viện", err);
    return unknown;
  }
}

/**
 * Nạp thêm tài liệu, tiến trình đi qua `Channel`.
 *
 * Trả về danh sách tài liệu **sau khi nạp**, không phải danh sách vừa thêm: một lô hai
 * mươi tệp có thể hỏng một tệp, và chỗ gọi cần bức tranh đúng của cả thư viện chứ không
 * cần ghép hai mảnh lại. Tệp hỏng đi ra qua `onProgress` với `error` khác `null`, còn
 * mười chín tệp kia vẫn vào — nên lệnh này chỉ ném khi *cả lô* không chạy được.
 */
export function addDocuments(
  paths: string[],
  onProgress: (p: IngestProgress) => void,
): Promise<DocumentView[]> {
  const channel = new Channel<IngestProgress>();
  channel.onmessage = onProgress;
  return invoke<DocumentView[]>("add_documents", { paths, onProgress: channel });
}

/** Bỏ một tài liệu khỏi thư viện, kèm mọi đoạn đã cắt từ nó. */
/**
 * Quét lại thư mục dự án.
 *
 * Chạy khi mở màn hình thư viện, không phải khi người dùng bấm một nút: thư mục là thư
 * viện, và bắt họ bấm "quét" để thấy tệp của chính mình là bắt họ làm việc của máy. Lõi
 * bỏ qua tệp không đổi nên lần chạy thứ hai gần như miễn phí.
 */
export function syncLibrary(onProgress: (p: IngestProgress) => void): Promise<DocumentView[]> {
  if (!inTauri()) return Promise.resolve([]);
  const channel = new Channel<IngestProgress>();
  channel.onmessage = onProgress;
  return invoke<DocumentView[]>("sync_library", { onProgress: channel });
}

export function removeDocument(id: string): Promise<void> {
  return invoke("remove_document", { id });
}

/**
 * Tìm thử trong thư viện.
 *
 * `limit` có mặc định vì một ô thử tìm không phải chỗ đọc hết kết quả: nó tồn tại để trả
 * lời "thư viện có tìm ra thứ này không", và câu trả lời đó nằm ở vài kết quả đầu.
 */
export function searchDocuments(query: string, limit = 8): Promise<DocumentHit[]> {
  return invoke<DocumentHit[]>("search_documents", { query, limit });
}

/**
 * Hộp thoại chọn tệp của hệ điều hành. Mảng rỗng = người dùng bấm huỷ.
 *
 * Vùng thả tệp là lối nhanh nhất nhưng không phải lối duy nhất được: kéo thả không có
 * ngoài Tauri, và không phải ai cũng có sẵn một cửa sổ Finder mở cạnh ứng dụng.
 *
 * Không lọc theo đuôi tệp ở đây. Danh sách định dạng đọc được nằm ở phía Rust, và một
 * bản sao ở giao diện sẽ lệch đúng vào lúc phía kia thêm một định dạng mới — người dùng
 * gặp một hộp thoại từ chối chính cái tệp mà lõi đọc được.
 */
export async function pickDocuments(): Promise<string[]> {
  if (!inTauri()) return [];
  const picked = await open({ directory: false, multiple: true });
  if (picked === null) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/* ── Vài hàm hiển thị, để bảng tài liệu không tự bịa cách gọi tên ───────────── */

const FORMAT_LABEL: Record<DocumentFormat, string> = {
  pdf: "PDF",
  docx: "Word",
  markdown: "Markdown",
  text: "Văn bản",
  html: "HTML",
  csv: "CSV",
  code: "Mã nguồn",
};

export function formatLabel(format: DocumentFormat): string {
  return FORMAT_LABEL[format];
}

/**
 * Kích thước tệp cho mắt người.
 *
 * Chia 1024 chứ không chia 1000, và làm tròn một chữ số thập phân từ mức MB trở lên:
 * dưới mức đó chữ số thập phân chỉ là nhiễu, còn từ mức đó trở lên "2 MB" và "2,4 MB" là
 * hai tệp khác nhau về thời gian nạp.
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/** Ba trạng thái nhúng, tách ra khỏi JSX vì cả bảng lẫn dải thống kê đều cần đọc nó. */
export type EmbedState = "embedded" | "queued" | "failed";

/**
 * `embedded === false` mà `error === null` là **đang xếp hàng**, không phải hỏng.
 *
 * Gộp hai thứ đó lại là lỗi tốn kém nhất của màn hình này: người dùng thấy một tệp hoàn
 * toàn bình thường bị đánh dấu hỏng sẽ xoá đi nạp lại, và lần nạp lại cũng "hỏng" y như
 * vậy cho tới khi hàng đợi chạy xong.
 */
export function embedState(doc: DocumentView): EmbedState {
  if (doc.error !== null) return "failed";
  return doc.embedded ? "embedded" : "queued";
}
