import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `docs` area. See lib/i18n/README.md for the wording rules. */
export const docs = {
  // Page header
  title: common.library,
  titleMore: {
    en: "In this kind of project the assistant never edits files and never runs commands.",
    vi: "Trợ lý không sửa tệp và không chạy lệnh trong dự án loại này.",
  },
  subtitle: { en: "The assistant reads these.", vi: "Trợ lý đọc tài liệu ở đây để trả lời." },
  loadingDocs: { en: "Reading library…", vi: "Đang đọc thư viện…" },

  // Library health strip
  stats: {
    documents: common.docs,
    chunks: { en: "Chunks", vi: "Đoạn" },
    embedded: { en: "Embedded", vi: "Đã nhúng" },
    embedder: { en: "Embedder", vi: "Bộ nhúng" },
    embedderNone: { en: "none yet", vi: "chưa có" },
    loading: { en: "Reading library health…", vi: "Đang đọc tình trạng thư viện…" },
    unknown: { en: "No library info yet.", vi: "Chưa có thông tin thư viện." },
    ready: {
      en: "Semantic and keyword search on.",
      vi: "Tìm theo ngữ nghĩa và từ khoá đều đang chạy.",
    },
    // Second half of the "not ready" strip: the bold part, then the rest of the sentence.
    keywordOn: { en: "Keyword search works", vi: "Tìm bằng từ khoá vẫn chạy" },
    keywordOnTail: { en: "— use it now.", vi: "— dùng được ngay." },
    keywordOnMore: {
      en: "Answers will catch more ways of phrasing once embedding finishes.",
      vi: "Câu trả lời sẽ bắt được nhiều cách diễn đạt hơn khi phần nhúng chạy xong.",
    },
  },

  // Reprocess button and the line beside it
  reprocess: {
    action: { en: "Reprocess", vi: "Xử lý lại" },
    busy: { en: "Working…", vi: "Đang xử lý…" },
    hint: { en: "Re-reads every file.", vi: "Đọc lại mọi tệp, kể cả tệp lần trước hỏng." },
    pendingOne: {
      en: "{n} chunk left to embed — click to finish.",
      vi: "Còn {n} đoạn chờ nhúng — bấm để nhúng nốt.",
    },
    pendingOther: {
      en: "{n} chunks left to embed — click to finish.",
      vi: "Còn {n} đoạn chờ nhúng — bấm để nhúng nốt.",
    },
    more: {
      en: "Re-reads every file in the folder, including files that failed last time and files unchanged since the last scan.",
      vi: "Đọc lại mọi tệp trong thư mục, kể cả tệp lần trước đọc hỏng và tệp không đổi từ lần quét trước.",
    },
  },

  // Ingest progress bar
  ingest: {
    progressLabel: { en: "Document ingest progress", vi: "Tiến trình nạp tài liệu" },
  },

  // List of files that failed to ingest
  failures: {
    none: { en: "No files were added.", vi: "Không tệp nào nạp được." },
    some: { en: "{ok} added, {bad} failed.", vi: "{ok} tệp đã vào, {bad} tệp không nạp được." },
    more: {
      en: "The library keeps working with the rest.",
      vi: "Thư viện vẫn dùng bình thường với phần còn lại.",
    },
    dismiss: {
      en: "Hide the list of files that failed",
      vi: "Ẩn danh sách tệp không nạp được",
    },
  },

  // Document table
  table: {
    caption: { en: "Documents in the library", vi: "Tài liệu trong thư viện" },
    document: { en: "Document", vi: "Tài liệu" },
    format: { en: "Format", vi: "Định dạng" },
    size: { en: "Size", vi: "Kích thước" },
    chunks: { en: "Chunks", vi: "Đoạn" },
    addedAt: { en: "Added", vi: "Nạp lúc" },
    embed: common.embedding,
    actions: { en: "Actions", vi: "Thao tác" },
    remove: {
      en: 'Remove "{title}" from the library',
      vi: 'Xoá "{title}" khỏi thư viện',
    },
  },

  // File format names; keys match `DocumentFormat`.
  format: {
    pdf: { en: "PDF", vi: "PDF" },
    docx: { en: "Word", vi: "Word" },
    markdown: { en: "Markdown", vi: "Markdown" },
    text: { en: "Text", vi: "Văn bản" },
    html: { en: "HTML", vi: "HTML" },
    csv: { en: "CSV", vi: "CSV" },
    code: { en: "Code", vi: "Mã nguồn" },
  },

  // Embedding status badge of a document
  embed: {
    embedded: { en: "Embedded", vi: "Đã nhúng" },
    queued: { en: "Queued", vi: "Đang xếp hàng" },
    failed: { en: "Failed", vi: "Hỏng" },
  },

  // Drop zone
  drop: {
    emptyTitle: { en: "Library empty", vi: "Thư viện còn trống" },
    emptyMore: {
      en: "Takes PDF, Word, Markdown, HTML, CSV and plain text. The original files stay where they are — the library only reads what is inside them.",
      vi: "Nhận PDF, Word, Markdown, HTML, CSV và văn bản thuần — tệp gốc nằm nguyên chỗ cũ, thư viện chỉ đọc nội dung.",
    },
    emptyHint: {
      en: "Drop files here, or browse.",
      vi: "Kéo tệp vào cửa sổ, hoặc chọn tệp từ máy.",
    },
    compactHint: { en: "Drop more files here.", vi: "Kéo tệp thả vào cửa sổ để nạp thêm." },
    pick: { en: "Choose files…", vi: "Chọn tệp…" },
  },

  // Search probe
  probe: {
    title: { en: "Test search", vi: "Thử tìm" },
    titleMore: {
      en: "This is search only — no answer is generated here.",
      vi: "Đây chỉ là tìm kiếm — không có câu trả lời nào được sinh ra ở đây.",
    },
    subtitle: { en: "Type a question to test.", vi: "Gõ câu hỏi để xem thư viện tìm ra gì." },
    placeholder: {
      en: "e.g. disaster recovery process",
      vi: "Ví dụ: quy trình khôi phục sau sự cố",
    },
    inputLabel: {
      en: "Question to test against the library",
      vi: "Câu hỏi để thử tìm trong thư viện",
    },
    busy: { en: "Searching…", vi: "Đang tìm…" },
    empty: { en: "No matching chunks.", vi: "Không tìm thấy đoạn nào khớp." },
    emptyMore: {
      en: "The library may hold nothing on this, or the question uses different words than the documents.",
      vi: "Thư viện có thể chưa có tài liệu về chuyện này, hoặc câu hỏi dùng từ khác với tài liệu.",
    },
    ordinal: { en: "chunk {n}", vi: "đoạn {n}" },
    quoteNote: {
      en: "Quoted verbatim from a document you added.",
      vi: "Trích nguyên văn từ tài liệu do bạn nạp lên.",
    },
    matchBoth: { en: "keyword + semantic", vi: "từ khoá + ngữ nghĩa" },
    matchSemantic: { en: "semantic", vi: "ngữ nghĩa" },
    matchKeyword: { en: "keyword", vi: "từ khoá" },
  },

  // Delete-document dialog: an irreversible action, so the warning is not shortened.
  remove: {
    title: { en: 'Remove "{title}" from the library?', vi: 'Xoá "{title}" khỏi thư viện?' },
    body: { en: "The file on disk stays untouched.", vi: "Tệp gốc trên đĩa vẫn nguyên." },
    more: {
      en: "The document and every chunk cut from it leave the library, so the assistant can no longer find this content. The original file on disk stays untouched — add it again any time.",
      vi: "Tài liệu và toàn bộ đoạn đã cắt từ nó bị bỏ khỏi thư viện, nên trợ lý sẽ không còn tìm thấy nội dung này nữa. Tệp gốc trên đĩa vẫn nguyên — nạp lại được bất cứ lúc nào.",
    },
    confirm: { en: "Remove from library", vi: "Xoá khỏi thư viện" },
  },

  // Error messages: they must name what broke, so they are not shortened.
  error: {
    scan: { en: "Could not scan the folder: {err}", vi: "Không quét được thư mục: {err}" },
    reprocess: {
      en: "Could not reprocess the library: {err}",
      vi: "Không xử lý lại được thư viện: {err}",
    },
    pick: common.pickerFailed,
    remove: { en: 'Could not delete "{title}": {err}', vi: 'Không xoá được "{title}": {err}' },
    stats: {
      en: "Could not read the library status.",
      vi: "Không hỏi được tình trạng thư viện.",
    },
  },

  /** Ingest stages, keyed by the exact string the core sends (`IngestStage::as_str`); `preparing` is UI-only
   * but lives here so `stage` only ever carries one kind of value. */
  stage: {
    preparing: common.preparing,
    reading: { en: "Reading", vi: "Đang đọc" },
    stored: { en: "Stored", vi: "Đã lưu" },
    failed: { en: "Failed", vi: "Hỏng" },
    skipped: { en: "Skipped", vi: "Bỏ qua" },
    removed: { en: "Removed", vi: "Đã bỏ" },
    embedding: { en: "Embedding", vi: "Đang nhúng" },
    finished: { en: "Finished", vi: "Xong" },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
