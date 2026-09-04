import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `projects` area. See lib/i18n/README.md for the wording rules. */
export const projects = {
  // Projects screen: page header
  title: common.projects,
  subtitle: { en: "One folder per project", vi: "Mỗi dự án là một thư mục trên máy." },
  scopeHint: {
    en: "The assistant sees only the folder of the open project.",
    vi: "Trợ lý chỉ nhìn thấy thư mục của dự án đang mở.",
  },

  // The three ways to create a project
  newCode: { en: "Code folder", vi: "Mở thư mục mã nguồn" },
  newCodeHint: {
    en: "Reads, edits, runs commands",
    vi: "Trợ lý đọc, sửa tệp và chạy lệnh ở đó.",
  },
  newDocs: { en: "Doc library", vi: "Tạo thư viện tài liệu" },
  newDocsHint: {
    en: "Ask docs, no edits",
    vi: "Nạp PDF, Word, Markdown… để hỏi đáp; không sửa tệp.",
  },
  cloneTitle: { en: "Clone repo", vi: "Clone từ Git" },
  cloneHint: { en: "Downloads a repo, opens it", vi: "Tải repo về máy rồi mở làm dự án." },

  // Search and filter
  searchPlaceholder: { en: "Search name or path", vi: "Tìm theo tên hoặc đường dẫn" },
  searchLabel: {
    en: "Search projects by name or path",
    vi: "Tìm dự án theo tên hoặc đường dẫn",
  },
  filterLabel: { en: "Filter by project kind", vi: "Lọc theo loại dự án" },
  filterAll: { en: "All", vi: "Tất cả" },

  // Project kind
  kindLabel: { en: "Project kind", vi: "Loại dự án" },
  kindCode: { en: "Code", vi: "Mã nguồn" },
  kindDocs: { en: "Doc library", vi: "Thư viện tài liệu" },
  kindCodeLong: { en: "Code project", vi: "Dự án mã nguồn" },
  /** Appears inside an accessibility label, so it is lower case. */
  kindCodeInline: { en: "code project", vi: "dự án mã nguồn" },
  kindDocsInline: { en: "doc library", vi: "thư viện tài liệu" },

  // Empty list
  emptyTitle: { en: "No projects", vi: "Chưa có dự án nào" },
  emptyHint: { en: "Open a folder to start", vi: "Mở một thư mục để bắt đầu." },
  noMatch: { en: "Nothing matches the filter", vi: "Không có dự án nào khớp bộ lọc." },

  // One project row
  current: { en: "Current", vi: "Đang mở" },
  rowOpenLabel: { en: 'Open "{name}"', vi: 'Mở "{name}"' },
  rowCurrentLabel: { en: '"{name}" is already open', vi: '"{name}" đang mở' },

  // Remove from the list: an irreversible action, so not shortened
  forgetTitle: {
    en: 'Remove "{name}" from the list?',
    vi: 'Bỏ "{name}" khỏi danh sách?',
  },
  forgetBody: {
    en: "The folder on disk is left untouched.",
    vi: "Thư mục trên đĩa không bị đụng tới.",
  },
  forgetMore: {
    en: "Only the recent-projects list changes. The folder and every file inside it stay on disk — open the folder again any time and the project comes back.",
    vi: "Chỉ danh sách dự án gần đây bị đổi. Thư mục và toàn bộ tệp bên trong vẫn nguyên trên đĩa — mở lại thư mục này bất cứ lúc nào là dự án trở lại.",
  },
  forgetConfirm: { en: "Remove from list", vi: "Bỏ khỏi danh sách" },
  // The heavier half of the same dialog: the two actions differ only in what is kept, so they are read side
  // by side rather than hidden behind separate buttons in the row.
  forgetOrDeleteBody: {
    en: 'The folder on disk is left untouched either way. "Delete its data" also drops this project\'s conversations and its indexed library.',
    vi: 'Thư mục trên đĩa không bị đụng tới, dù chọn cách nào. "Xoá cả dữ liệu" bỏ thêm phiên trò chuyện và thư viện đã lập chỉ mục của dự án này.',
  },
  deleteConfirm: { en: "Delete its data", vi: "Xoá cả dữ liệu" },
  deleteBusy: { en: "Deleting…", vi: "Đang xoá…" },
  forgetLabel: {
    en: 'Remove "{name}" from the list',
    vi: 'Bỏ "{name}" khỏi danh sách',
  },
  forgetBlockedLabel: {
    en: '"{name}" is open and cannot be removed',
    vi: '"{name}" đang mở, không bỏ được',
  },

  // File table of the open project
  panelLabel: { en: "Files in {name}", vi: "Tệp {name}" },
  panelTitle: { en: "Project files", vi: "Tệp trong dự án" },
  panelMeta: { en: "{kind} · opened {when}", vi: "{kind} · mở {when}" },
  openLibrary: { en: "Open the doc library", vi: "Mở Thư viện tài liệu" },
  openChanges: { en: "Open the changes screen", vi: "Mở màn hình Thay đổi" },
  openInFileManager: {
    en: "Open the folder in the file manager",
    vi: "Mở thư mục trong trình quản lý tệp",
  },
  closePanel: { en: "Close the files panel", vi: "Đóng bảng tệp" },
  reading: { en: "reading…", vi: "đang đọc…" },
  emptyDir: { en: "empty folder", vi: "thư mục rỗng" },
  pickFileTip: {
    en: "Put {name} into the composer",
    vi: "Đưa {name} vào ô soạn tin",
  },
  reindexFile: { en: "Re-index {name}", vi: "Lập chỉ mục lại {name}" },
  reindexingFile: { en: "Re-indexing {name}", vi: "Đang lập chỉ mục lại {name}" },
  reindexedFile: { en: "Re-indexed {name}", vi: "Đã lập chỉ mục lại {name}" },
  retryReindexFile: {
    en: "Retry re-indexing {name}",
    vi: "Thử lập chỉ mục lại {name}",
  },
  reindexPreparing: common.preparing,
  reindexError: {
    en: "Could not re-index {name}: {err}",
    vi: "Không lập chỉ mục lại được {name}: {err}",
  },
  uploadFiles: { en: "Upload files", vi: "Tải tệp lên" },
  uploadTitle: { en: "Upload files", vi: "Tải tệp lên" },
  uploadDesc: {
    en: "Copy files into {name}",
    vi: "Chép tệp vào {name}",
  },
  uploadMore: {
    en: "Files are copied into the project root. Existing files are never replaced, and folders are not accepted.",
    vi: "Tệp được chép vào thư mục gốc của dự án. Tệp đã có sẽ không bị ghi đè và không nhận cả thư mục.",
  },
  uploadDropTitle: { en: "Drop files here", vi: "Thả tệp vào đây" },
  uploadDropHint: {
    en: "Drop files or choose them from your device",
    vi: "Thả tệp hoặc chọn từ máy của bạn",
  },
  uploadChoose: { en: "Choose files", vi: "Chọn tệp" },
  uploading: { en: "Uploading files…", vi: "Đang tải tệp lên…" },
  uploadDoneOne: { en: "Uploaded {n} file", vi: "Đã tải lên {n} tệp" },
  uploadDoneMany: { en: "Uploaded {n} files", vi: "Đã tải lên {n} tệp" },
  uploadError: {
    en: "Could not upload files: {err}",
    vi: "Không tải được tệp: {err}",
  },

  // New project dialog
  newTitle: { en: "New project", vi: "Dự án mới" },
  newDesc: { en: "Point to an existing folder", vi: "Trỏ vào một thư mục đã có trên máy." },
  newMore: {
    en: "No file is created or changed inside that folder.",
    vi: "Không có tệp nào bị tạo hay sửa trong thư mục đó.",
  },
  create: { en: "Create", vi: "Tạo dự án" },
  opening: { en: "Opening project…", vi: "Đang mở dự án…" },
  folder: { en: "Folder", vi: "Thư mục" },
  folderPlaceholder: { en: "/Users/you/Workspaces/project", vi: "/Users/ban/Workspaces/du-an" },
  choose: { en: "Choose…", vi: "Chọn…" },
  dropHint: { en: "Dragging a folder works too", vi: "Kéo một thư mục vào cửa sổ cũng điền được." },
  pickCode: { en: "Choose a code folder", vi: "Chọn thư mục mã nguồn" },
  pickDocs: { en: "Choose a docs folder", vi: "Chọn thư mục tài liệu" },
  pickParent: { en: "Choose the parent folder", vi: "Chọn thư mục cha" },
  pickError: {
    en: "Could not open the folder picker: {err}",
    vi: "Không mở được hộp thoại chọn thư mục: {err}",
  },

  // The two kind cards
  kindCodeCan: { en: "Reads, edits, runs commands", vi: "Trợ lý đọc, sửa tệp và chạy lệnh." },
  kindCodeCannot: { en: "Every write asks first", vi: "Mỗi thao tác ghi đều hỏi ý bạn trước." },
  kindDocsCan: {
    en: "Searches and reads your docs",
    vi: "Trợ lý tìm và đọc tài liệu để trả lời.",
  },
  kindDocsCannot: { en: "No edits, no commands", vi: "Không sửa tệp, không chạy lệnh." },
  kindWarn: {
    en: "Changing kind recreates the project",
    vi: "Đổi loại sau này nghĩa là tạo lại dự án.",
  },
  kindMore: {
    en: "With the wrong kind the assistant simply cannot edit files, and never says why. Changing the kind later means recreating the project — the folder itself stays.",
    vi: "Chọn nhầm loại thì trợ lý sẽ không sửa được tệp mà không nói rõ vì sao. Đổi loại sau này nghĩa là tạo lại dự án — thư mục thì vẫn nguyên.",
  },

  // Clone dialog
  cloneDesc: { en: "Downloads a repo, opens it", vi: "Tải một repo về máy rồi mở làm dự án." },
  clone: { en: "Clone", vi: "Clone" },
  cloning: { en: "Cloning…", vi: "Đang clone…" },
  cancelClone: { en: "Cancel clone", vi: "Huỷ clone" },
  clonePreparing: common.preparing,
  cloneCancelling: { en: "Cancelling…", vi: "Đang huỷ…" },
  cloneCancelled: { en: "Cancelled", vi: "Đã huỷ" },
  cloneProgress: { en: "Cloning: {phase}", vi: "Đang clone: {phase}" },
  repoUrl: { en: "Repo URL", vi: "URL repo" },
  repoUrlPlaceholder: {
    en: "https://github.com/name/repo.git",
    vi: "https://github.com/ten/repo.git",
  },
  parentFolder: { en: "Parent folder", vi: "Thư mục cha" },
  parentPlaceholder: { en: "/Users/you/Workspaces", vi: "/Users/ban/Workspaces" },
  folderName: { en: "Folder name", vi: "Tên thư mục" },
  shallow: { en: "Recent history only", vi: "Chỉ lấy lịch sử gần nhất" },
  shallowLabel: { en: "About recent history only", vi: "Về việc chỉ lấy lịch sử gần nhất" },
  shallowHint: { en: "Faster, enough to read code", vi: "Nhanh hơn nhiều và đủ để đọc mã." },
  shallowMore: {
    en: "Much faster, and enough to read the code. In exchange, older history is not there and you cannot push back to another branch — clone again in full when you need that.",
    vi: "Nhanh hơn nhiều và đủ để đọc mã. Đổi lại, không xem được lịch sử cũ và không đẩy ngược lên nhánh khác — cần thì clone lại đầy đủ.",
  },
  details: { en: "Details", vi: "Chi tiết" },
  lineOne: { en: "{n} line", vi: "{n} dòng" },
  lineMany: common.linesMany,

  // Project groups in the sidebar
  listEmpty: { en: "No projects yet", vi: "Danh sách chưa có dự án nào." },
  rowMenu: { en: "Options for {name}", vi: "Tuỳ chọn cho dự án {name}" },
  rowCurrentA11y: {
    en: "{name} — {kind}, the open project. {path}",
    vi: "{name} — {kind}, dự án đang mở. {path}",
  },
  rowOpenA11y: { en: "Open {kind} {name}. {path}", vi: "Mở {kind} {name}. {path}" },
  openThis: { en: "Open", vi: "Mở dự án này" },
  toDocs: { en: "Switch to docs", vi: "Chuyển thành thư viện tài liệu" },
  toDocsHint: {
    en: "Read-only; no edits or commands",
    vi: "Thôi sửa tệp và chạy lệnh, chỉ đọc tài liệu.",
  },
  toCode: { en: "Switch to code", vi: "Chuyển thành dự án mã nguồn" },
  toCodeHint: { en: "Reads, edits, runs commands", vi: "Trợ lý đọc, sửa tệp và chạy được lệnh." },
  closeProject: { en: "Close project", vi: "Đóng dự án, chỉ trò chuyện" },
  closeProjectHint: {
    en: "Stays listed; files go unread",
    vi: "Vẫn ở trong danh sách; trợ lý thôi đọc tệp.",
  },
  forgetBlockedHint: { en: "Open — close it first", vi: "Đang mở — đóng dự án trước đã." },
  forgetSafeHint: {
    en: "The folder on disk stays intact, no file is lost.",
    vi: "Thư mục trên đĩa vẫn nguyên, không tệp nào mất.",
  },
  showMore: { en: "{n} more", vi: "Xem thêm {n} dự án" },
  collapse: { en: "Show less", vi: "Thu gọn" },
  seeAll: { en: "All projects…", vi: "Tất cả dự án…" },
  noProjectNote: { en: "No project — chat only", vi: "Chưa mở dự án — trợ lý chỉ trò chuyện." },
  noProjectLabel: { en: "About having no project open", vi: "Về trạng thái chưa có dự án" },
  noProjectMore: {
    en: "No project open — the assistant only chats, and reads no file. Open a code project and the Changes screen appears; open a doc library and the Library screen appears. A project has exactly one kind, so those two screens never show up together.",
    vi: "Chưa mở dự án — trợ lý chỉ trò chuyện, không đọc tệp. Mở một dự án mã nguồn thì có thêm màn hình Thay đổi; mở một thư viện tài liệu thì có thêm màn hình Thư viện. Một dự án chỉ thuộc một loại, nên hai màn hình đó không bao giờ cùng xuất hiện.",
  },
  switching: common.switchingProject,
} satisfies Record<string, Msg | Record<string, Msg>>;
