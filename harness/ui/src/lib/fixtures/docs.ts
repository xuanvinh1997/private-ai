import type { DocumentHit, DocumentView, IngestProgress, LibraryStats } from "../protocol";

/** Sample data for the document library under `?demo=1`; all three embedding states appear side by side, and
 * `demoLibraryStats()` deliberately reports `semanticReady: false`, which keyword search survives. */

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
      pages: 40,
      ocrPages: [3, 4, 12],
      embedded: true,
      addedAt: now - 4 * DAY,
      error: null,
    },
    {
      id: "d-hop-dong",
      path: "/Users/vinhpx/Documents/so-tay/hop-dong-mau.docx",
      title: "Hợp đồng mẫu 2026",
      format: "office",
      bytes: 148_992,
      chunks: 26,
      pages: 0,
      ocrPages: [],
      embedded: true,
      addedAt: now - 2 * DAY,
      error: null,
    },
    {
      // Queued: no vector yet but NO error. This row is the eyeball test.
      id: "d-bien-ban",
      path: "/Users/vinhpx/Documents/so-tay/bien-ban-hop-q3.md",
      title: "Biên bản họp quý III",
      format: "markdown",
      bytes: 31_744,
      chunks: 12,
      pages: 0,
      ocrPages: [],
      embedded: false,
      addedAt: now - 40 * MINUTE,
      error: null,
    },
    {
      id: "d-bang-gia",
      path: "/Users/vinhpx/Documents/so-tay/bang-gia.csv",
      title: "Bảng giá dịch vụ",
      format: "data",
      bytes: 9_216,
      chunks: 4,
      pages: 0,
      ocrPages: [],
      embedded: false,
      addedAt: now - 12 * MINUTE,
      error: null,
    },
    {
      // Failed, with a reason specific enough to act on rather than the word "error".
      id: "d-ban-quet",
      path: "/Users/vinhpx/Documents/so-tay/ban-quet-2019.pdf",
      title: "Bản quét lưu trữ 2019",
      format: "pdf",
      bytes: 18_874_368,
      chunks: 0,
      pages: 28,
      ocrPages: [],
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
      pages: 0,
      ocrPages: [],
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
    root: "/Users/vinhpx/Documents/NCS",
    filesSeen: 8,
    // Two size-capped files and one unreadable, so the gap between "8 files in the folder" and "6 in the library" shows.
    filesSkipped: 1,
    unreadable: 1,
    excluded: 0,
    scannedAt: Date.now() - 4 * 60_000,
    scanning: null,
  };
}

/** A fake ingest batch where one file fails and the rest succeed, the shape a UI most often gets wrong. */
export function demoIngestFrames(paths: string[]): IngestProgress[] {
  const total = paths.length;
  return paths.flatMap((path, at): IngestProgress[] => {
    const done = at + 1;
    // The second file of every batch fails, so there is always one failure to look at.
    const broken = at === 1;
    return [
      { path, stage: "reading", done: at, total, finished: false, error: null },
      broken
        ? {
            path,
            stage: "skipped",
            done,
            total,
            finished: false,
            error: "Định dạng không đọc được — tệp có thể đã hỏng hoặc đang bị khoá.",
          }
        : { path, stage: "stored", done, total, finished: false, error: null },
    ];
  });
}

/** Probe results covering all three `matchedBy` values, the badges that explain why one hit ranked and another did not. */
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
