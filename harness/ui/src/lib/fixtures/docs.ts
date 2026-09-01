import type { DocumentHit, DocumentView, IngestProgress, LibraryStats } from "../protocol";

/**
 * Dữ liệu mẫu cho màn hình thư viện tài liệu ở chế độ `?demo=1`.
 *
 * Ba trạng thái nhúng đều có mặt, và đó là lý do chính tệp này tồn tại: *đã nhúng*,
 * *đang xếp hàng*, và *hỏng kèm lý do* trông rất giống nhau trong mã nguồn mà phải trông
 * rất khác nhau trên màn hình. Không đặt cả ba cạnh nhau thì không ai kiểm được điều đó.
 *
 * `demoLibraryStats()` cố ý trả `semanticReady: false`: một thư viện chưa nhúng xong vẫn
 * tìm được bằng từ khoá, và dải trạng thái phải nói ra điều đó thay vì để người dùng
 * tưởng thư viện hỏng và bỏ cuộc.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function demoDocuments(now = Date.now()): DocumentView[] {
  return [
    {
      id: "d-so-tay",
      path: "/Users/vinhpx/Documents/so-tay/so-tay-van-hanh.pdf",
      title: "Sổ tay vận hành hệ thống",
      format: "pdf",
      bytes: 2_412_544,
      chunks: 184,
      embedded: true,
      addedAt: now - 4 * DAY,
      error: null,
    },
    {
      id: "d-hop-dong",
      path: "/Users/vinhpx/Documents/so-tay/hop-dong-mau.docx",
      title: "Hợp đồng mẫu 2026",
      format: "docx",
      bytes: 148_992,
      chunks: 26,
      embedded: true,
      addedAt: now - 2 * DAY,
      error: null,
    },
    {
      // Đang xếp hàng: chưa có vector nhưng KHÔNG có lỗi. Tệp này là bài kiểm cho mắt.
      id: "d-bien-ban",
      path: "/Users/vinhpx/Documents/so-tay/bien-ban-hop-q3.md",
      title: "Biên bản họp quý III",
      format: "markdown",
      bytes: 31_744,
      chunks: 12,
      embedded: false,
      addedAt: now - 40 * MINUTE,
      error: null,
    },
    {
      id: "d-bang-gia",
      path: "/Users/vinhpx/Documents/so-tay/bang-gia.csv",
      title: "Bảng giá dịch vụ",
      format: "csv",
      bytes: 9_216,
      chunks: 4,
      embedded: false,
      addedAt: now - 12 * MINUTE,
      error: null,
    },
    {
      // Hỏng, kèm lý do đủ để người đọc biết phải làm gì tiếp — chứ không phải "lỗi".
      id: "d-ban-quet",
      path: "/Users/vinhpx/Documents/so-tay/ban-quet-2019.pdf",
      title: "Bản quét lưu trữ 2019",
      format: "pdf",
      bytes: 18_874_368,
      chunks: 0,
      embedded: false,
      addedAt: now - 3 * HOUR,
      error: "Tệp PDF chỉ có ảnh quét, không rút được chữ nào. Cần OCR trước khi nạp.",
    },
    {
      id: "d-kien-truc",
      path: "/Users/vinhpx/Documents/so-tay/kien-truc.html",
      title: "Ghi chú kiến trúc",
      format: "html",
      bytes: 64_512,
      chunks: 31,
      embedded: true,
      addedAt: now - 9 * DAY,
      error: null,
    },
  ];
}

export function demoLibraryStats(): LibraryStats {
  return {
    documents: 6,
    chunks: 257,
    embeddedChunks: 241,
    embedder: "nomic-embed-text",
    semanticReady: false,
    reason: "Còn 16 đoạn chưa nhúng xong.",
  };
}

/**
 * Một lô nạp giả, trong đó **một tệp hỏng còn phần còn lại vẫn vào**.
 *
 * Đây là hình dạng thật của việc nạp tài liệu, và cũng là hình dạng dễ vẽ sai nhất: một
 * giao diện báo "thất bại" cho cả lô vì một tệp hỏng sẽ khiến người dùng nạp lại mười
 * chín tệp đã nằm sẵn trong thư viện.
 */
export function demoIngestFrames(paths: string[]): IngestProgress[] {
  const total = paths.length;
  return paths.flatMap((path, at): IngestProgress[] => {
    const done = at + 1;
    // Tệp thứ hai của mỗi lô hỏng — luôn có một tệp hỏng để nhìn, kể cả lô hai tệp.
    const broken = at === 1;
    return [
      { path, stage: "Đang đọc", done: at, total, finished: false, error: null },
      broken
        ? {
            path,
            stage: "Bỏ qua",
            done,
            total,
            finished: false,
            error: "Định dạng không đọc được — tệp có thể đã hỏng hoặc đang bị khoá.",
          }
        : { path, stage: "Đang cắt đoạn", done, total, finished: false, error: null },
    ];
  });
}

/**
 * Kết quả tìm thử.
 *
 * Đủ cả ba `matchedBy`: chỉ từ khoá, chỉ ngữ nghĩa, và cả hai. Ba huy hiệu đó là thứ
 * giải thích vì sao một câu hỏi tìm ra kết quả này mà không tìm ra kết quả kia, nên cả
 * ba phải được nhìn thấy cạnh nhau ít nhất một lần.
 */
export function demoHits(query: string): DocumentHit[] {
  const q = query.trim();
  if (q === "") return [];
  return [
    {
      documentId: "d-so-tay",
      title: "Sổ tay vận hành hệ thống",
      path: "/Users/vinhpx/Documents/so-tay/so-tay-van-hanh.pdf",
      ordinal: 42,
      text: `Quy trình khôi phục sau sự cố: dừng dịch vụ, khôi phục bản sao lưu gần nhất, đối chiếu nhật ký, rồi mới mở lại cổng ngoài. Không bỏ qua bước đối chiếu — “${q}” nằm ở đây.`,
      score: 0.91,
      matchedBy: "both",
    },
    {
      documentId: "d-hop-dong",
      title: "Hợp đồng mẫu 2026",
      path: "/Users/vinhpx/Documents/so-tay/hop-dong-mau.docx",
      ordinal: 7,
      text: "Bên B chịu trách nhiệm bảo mật toàn bộ dữ liệu do Bên A cung cấp trong suốt thời hạn hợp đồng và ba năm sau khi hợp đồng chấm dứt.",
      score: 0.74,
      matchedBy: "semantic",
    },
    {
      documentId: "d-bien-ban",
      title: "Biên bản họp quý III",
      path: "/Users/vinhpx/Documents/so-tay/bien-ban-hop-q3.md",
      ordinal: 3,
      text: `Kết luận: hoãn việc chuyển kho sang cụm mới tới quý IV, giữ nguyên lịch sao lưu hằng đêm. Từ khoá đã khớp: ${q}.`,
      score: 0.58,
      matchedBy: "keyword",
    },
  ];
}
