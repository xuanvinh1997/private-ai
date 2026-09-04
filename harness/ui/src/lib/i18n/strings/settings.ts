import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `settings` area. See lib/i18n/README.md for the wording rules. */
export const settings = {
  // Screen frame: sidebar, search box, exit.
  shell: {
    backToApp: { en: "Back to app", vi: "Về ứng dụng" },
    searchPlaceholder: { en: "Search settings…", vi: "Tìm trong cài đặt…" },
    searchLabel: { en: "Search settings", vi: "Tìm trong cài đặt" },
    navLabel: { en: "Settings sections", vi: "Mục cài đặt" },
    sidebarLabel: common.settings,
  },

  // Search results.
  search: {
    title: { en: "Results", vi: "Kết quả tìm" },
    none: {
      en: "No setting matches “{q}”.",
      vi: "Không có mục cài đặt nào khớp “{q}”.",
    },
    hitOne: { en: "{n} setting matches “{q}”.", vi: "{n} mục khớp “{q}”." },
    hitMany: { en: "{n} settings match “{q}”.", vi: "{n} mục khớp “{q}”." },
    scope: {
      en: "Settings only, not your data.",
      vi: "Ô tìm chỉ thấy mục cài đặt, không thấy dữ liệu trên máy bạn.",
    },
  },

  // Sidebar groups; the first group has no name.
  group: {
    integrations: { en: "Integrations", vi: "Tích hợp" },
    advanced: { en: "Advanced", vi: "Nâng cao" },
  },

  // Title and description of each page; keys match `SettingsPage`.
  page: {
    provider: { en: "Models", vi: "Mô hình" },
    chung: { en: "General", vi: "Chung" },
    chungDesc: { en: "Colors and chat layout", vi: "Bảng màu và cách hội thoại được vẽ ra." },
    phimTat: { en: "Shortcuts", vi: "Phím tắt" },
    phimTatDesc: { en: "Reference only, not rebindable", vi: "Bảng tra cứu, chưa gán lại phím được." },
    mcp: common.mcpServers,
    hook: { en: "Hooks", vi: "Hook" },
    hookDesc: { en: "Your commands before tool calls", vi: "Lệnh của bạn chạy trước mỗi lời gọi tool." },
    quyen: { en: "Permissions", vi: "Quyền" },
    quyenDesc: { en: "What the assistant may do", vi: "Trợ lý được phép làm gì trên máy này." },
  },

  // Search index: one row per setting, including those inside dialogs.
  index: {
    themeLabel: { en: "Theme", vi: "Bảng màu" },
    themeDesc: {
      en: "Light, dark, or system",
      vi: "Sáng, tối, hoặc theo hệ thống; đổi là thấy ngay.",
    },
    localeLabel: { en: "Language", vi: "Ngôn ngữ" },
    localeDesc: { en: "English or Vietnamese", vi: "Tiếng Anh hoặc tiếng Việt." },
    layoutLabel: { en: "Chat layout", vi: "Cách hiển thị hội thoại" },
    layoutDesc: {
      en: "Bubbles or full-width document",
      vi: "Bong bóng hai bên, hoặc tài liệu trải hết trang.",
    },
    findSessionLabel: common.findSession,
    findSessionDesc: { en: "⌘K or Ctrl+K", vi: "⌘K hoặc Ctrl+K mở bảng chọn phiên." },
    sendLabel: common.sendMessage,
    sendDesc: { en: "Enter sends, Shift+Enter breaks", vi: "Enter gửi, Shift+Enter xuống dòng." },
    providerLabel: { en: "Model provider", vi: "Nhà cung cấp mô hình" },
    providerDesc: {
      en: "Ollama, or a remote API",
      vi: "Máy chủ hội thoại: Ollama, hoặc một API từ xa.",
    },
    apiKeyLabel: common.apiKey,
    apiKeyDesc: {
      en: "Sent with every remote request",
      vi: "Khoá gửi kèm mỗi yêu cầu tới dịch vụ từ xa.",
    },
    baseUrlLabel: common.baseUrl,
    baseUrlDesc: {
      en: "Where questions are sent",
      vi: "Base URL của máy chủ mô hình, nơi câu hỏi tới.",
    },
    embedModelLabel: common.embeddingModel,
    embedModelDesc: {
      en: "Turns documents into vectors",
      vi: "Mô hình biến tài liệu thành vector để tìm nghĩa.",
    },
    chatModelLabel: common.chatModel,
    chatModelDesc: {
      en: "Answers what you type",
      vi: "Mô hình trả lời câu hỏi của bạn trong ô soạn tin.",
    },
    rerankLabel: common.rerank,
    rerankDesc: {
      en: "Reorders hits; off is faster",
      vi: "Sắp lại thứ tự đoạn tìm được; tắt đi thì tìm nhanh hơn.",
    },
    rerankDepthLabel: { en: "Rerank depth", vi: "Số đoạn chấm lại" },
    rerankDepthDesc: {
      en: "More passages, better order, slower",
      vi: "Càng nhiều đoạn thì thứ tự càng đúng và càng chờ lâu.",
    },
    mcpLabel: common.mcpServers,
    mcpDesc: { en: "Outside tools: repos, databases", vi: "Cắm tool từ ngoài: kho mã, cơ sở dữ liệu." },
    hookLabel: { en: "Pre-tool hook", vi: "Hook trước lời gọi tool" },
    hookDesc: {
      en: "Runs before each tool call",
      vi: "Lệnh ngoài chạy trước mỗi lời gọi tool và chặn được.",
    },
    scopeLabel: { en: "Tool scope", vi: "Phạm vi tool cho lượt mới" },
    scopeDesc: {
      en: "Read, write, or run commands",
      vi: "Lượt mới bắt đầu ở mức đọc, ghi, hay chạy lệnh.",
    },
    sandboxLabel: { en: "Sandbox", vi: "Vòng giam tiến trình" },
    sandboxDesc: {
      en: "Blocks writes outside the project",
      vi: "Sandbox chặn lệnh ghi ra ngoài thư mục dự án.",
    },
  },

  // General page.
  general: {
    displayTitle: { en: "Display", vi: "Hiển thị" },
    displayDesc: { en: "Applied now, remembered later", vi: "Đổi là thấy ngay, và được nhớ cho lần sau." },
    theme: { en: "Theme", vi: "Bảng màu" },
    themeDesc: { en: "Light, dark, or system", vi: "Sáng, tối, hoặc đi theo hệ thống." },
    themeMore: {
      en: "Follows the system theme.",
      vi: "Theo hệ thống.",
    },
    themeLight: { en: "Light", vi: "Sáng" },
    themeDark: { en: "Dark", vi: "Tối" },
    themeSystem: common.themeSystem,
    locale: { en: "Language", vi: "Ngôn ngữ" },
    localeDesc: { en: "The language of this app", vi: "Ngôn ngữ của toàn bộ giao diện." },
    localeMore: {
      en: "Applies to every screen right away, and is remembered on this machine. It never follows the system language on its own.",
      vi: "Áp cho mọi màn hình ngay lập tức và được nhớ trên máy này. Nó không tự đi theo ngôn ngữ của hệ điều hành.",
    },
    layout: { en: "Chat layout", vi: "Cách hiển thị hội thoại" },
    layoutDesc: {
      en: "Bubbles or full-width document",
      vi: "Bong bóng hai bên, hoặc tài liệu trải hết trang.",
    },
    layoutMore: {
      en: "Document mode drops the bubbles and spans the full width — easier to read for long diffs and wide tables.",
      vi: "Chế độ tài liệu bỏ bong bóng và trải hết bề rộng — dễ đọc hơn với diff dài và bảng rộng.",
    },
    layoutBubble: { en: "Bubbles", vi: "Bong bóng" },
    layoutDocument: { en: "Document", vi: "Tài liệu" },
  },

  // Hooks page.
  hooks: {
    warnTitle: { en: "Read first", vi: "Ba điều phải biết trước" },
    warnDesc: { en: "Opposite of what you expect", vi: "Cả ba đều ngược với chữ “hook bảo mật”." },

    failOpen: { en: "Failure allows", vi: "Hook hỏng thì cho qua" },
    failOpenDesc: { en: "A broken hook still allows", vi: "Hook lỗi thì lời gọi vẫn chạy." },
    failOpenMore: {
      en: "A syntax error, a timeout or a missing file is a fault in the policy, not evidence that the call is dangerous — so the call still runs. The approval dialog is the opposite: no answer means refused.",
      vi: "Hook lỗi cú pháp, hết giờ hay thiếu tệp đều là lỗi của chính sách, không phải bằng chứng rằng lời gọi nguy hiểm — nên lời gọi vẫn chạy. Hộp thoại duyệt thì ngược lại: không trả lời được là từ chối.",
    },
    failOpenTag: { en: "fail-open", vi: "fail-open" },

    unsandboxed: { en: "Outside the sandbox", vi: "Hook chạy ngoài vòng giam" },
    unsandboxedDesc: { en: "Runs with your full rights", vi: "Hook chạy với đầy đủ quyền của bạn." },
    unsandboxedMore: {
      en: "A hook is spawned directly, not through the Shell seam, so it runs with your full rights. Letting the assistant's sandbox decide whether the policy may run inverts the relationship.",
      vi: "Hook được spawn thẳng, không qua seam Shell, nên nó chạy với đầy đủ quyền của bạn. Để vòng giam của trợ lý quyết định chính sách có được chạy hay không là lộn ngược quan hệ.",
    },
    unsandboxedTag: { en: "full rights", vi: "đầy đủ quyền" },

    noRewrite: { en: "No rewrites", vi: "Hook không sửa được tham số" },
    noRewriteDesc: { en: "Only allow or deny", vi: "Chỉ allow hoặc deny, không viết lại tham số." },
    noRewriteMore: {
      en: "Only allow or deny. Rewriting arguments sounds handy, but it builds a call neither you nor the model ever saw, and the transcript then lies about what ran.",
      vi: "Chỉ allow hoặc deny. Viết lại tham số nghe tiện, nhưng nó tạo ra một lời gọi mà cả mô hình lẫn bạn đều không thấy, và bản ghi sẽ nói dối về thứ đã chạy.",
    },
    noRewriteTag: { en: "block only", vi: "chỉ chặn" },

    listTitle: { en: "Installed hooks", vi: "Hook đang cài" },
    listDesc: { en: "Read from the applied config", vi: "Đọc từ hàng cấu hình đã áp lớp." },
    listMore: {
      en: "Read from the applied config row — the command, the tools it covers, and its own timeout if it has one.",
      vi: "Đọc từ hàng cấu hình đã áp lớp — lệnh, tool nó áp vào, và hạn giờ riêng nếu có.",
    },
    listEmpty: { en: "No hooks", vi: "Chưa cài hook nào" },
    listEmptyDesc: { en: "The default; nothing intercepts", vi: "Đây là mặc định, không gì chen vào giữa." },
    listEmptyMore: {
      en: "This is the default. A hook is a command that runs before every tool call, so no hooks means nothing sits in between.",
      vi: "Đây là mặc định. Mỗi hook là một lệnh chạy trước mỗi lời gọi tool, nên không có hook nghĩa là không có gì chen vào giữa.",
    },
    itemAllTools: { en: "All tools", vi: "Áp cho mọi tool" },
    itemSomeTools: { en: "Only: {list}", vi: "Chỉ áp cho: {list}" },
    itemLine: {
      en: "{tools} · {secs}s timeout · declared in {origin}",
      vi: "{tools} · hạn giờ {secs} giây · khai ở {origin}",
    },

    rowLabel: { en: "`hooks` row", vi: "Hàng `hooks` trong cây plugin" },
    stateDemo: { en: "—", vi: "—" },
    stateDemoDesc: { en: "The demo build has no core", vi: "Bản demo không có lõi để hỏi." },
    stateAsking: { en: "asking…", vi: "đang hỏi…" },
    stateAskingDesc: { en: "Asking the core…", vi: "Đang hỏi lõi…" },
    stateError: { en: "error", vi: "lỗi" },
    stateErrorDesc: { en: "The core did not answer.", vi: "Không hỏi được lõi." },
    stateMissing: { en: "missing", vi: "vắng" },
    stateMissingDesc: {
      en: "No `hooks` row in the running tree.",
      vi: "Không có hàng `hooks` trong cây đang chạy.",
    },
    stateMissingMore: {
      en: "There is no `hooks` row in the running tree at all, so no hook can run.",
      vi: "Không có hàng `hooks` nào trong cây đang chạy, nên không hook nào chạy được.",
    },
    stateEmpty: { en: "empty", vi: "rỗng" },
    stateEmptyDesc: {
      en: "Still built-in: no hooks declared.",
      vi: "Vẫn như bản dựng sẵn: danh sách hook rỗng.",
    },
    stateEmptyMore: {
      en: "Exactly as built in, which means an empty hook list. No hook runs on this machine.",
      vi: "Vẫn đúng như bản dựng sẵn, tức là danh sách hook rỗng. Chưa có hook nào chạy trên máy này.",
    },
    statePatched: { en: "patched", vi: "có vá" },
    statePatchedDesc: {
      en: "Patched by a config layer: {origin}.",
      vi: "Đã bị một lớp cấu hình vá vào: {origin}.",
    },
    statePatchedMore: {
      en: "One of your config layers patched it: {origin}. So the patch file does declare hooks — but the diagnostic command does not say which.",
      vi: "Đã bị một lớp cấu hình của bạn vá vào: {origin}. Nghĩa là tệp vá có khai hook — nhưng khai những gì thì lệnh chẩn đoán không nói ra.",
    },

    readOnlyTitle: { en: "Read only", vi: "Đọc được, chưa sửa được từ đây" },
    readOnlyBody: {
      en: "Configure hooks by editing the patch file by hand.",
      vi: "Cấu hình hook bằng cách sửa tay tệp vá.",
    },
    readOnlyMore: {
      en: "The core can list installed hooks, but has no command to add, edit or remove one — so this screen builds no form. A form calling commands that do not exist throws on every click. Until that command exists, hooks are configured by editing the patch file.",
      vi: "Lõi đã liệt kê được hook đang cài, nhưng chưa có lệnh nào thêm, sửa hay xoá — nên màn hình này không dựng biểu mẫu. Một biểu mẫu gọi vào lệnh không tồn tại thì mọi cú bấm đều ném lỗi. Cho tới lúc có lệnh ấy, hook cấu hình bằng cách sửa tay tệp vá.",
    },

    manualTitle: { en: "Edit by hand", vi: "Sửa bằng tay" },
    manualDesc: { en: "Restart the app afterwards", vi: "Sửa xong phải mở lại ứng dụng." },
    manualMore: {
      en: "Restart the app afterwards: the plugin tree is built once at startup.",
      vi: "Sửa xong phải mở lại ứng dụng: cây plugin được dựng một lần lúc khởi động.",
    },
    fileDesc: { en: "The only place hooks live", vi: "Chỗ duy nhất khai được hook." },
    fileMore: {
      en: "The only place hooks can be declared. This is the default path — set PAI_DATA_DIR and the file lives in that directory instead.",
      vi: "Chỗ duy nhất khai được hook. Đây là đường dẫn mặc định — đặt biến môi trường PAI_DATA_DIR thì tệp nằm trong thư mục đó.",
    },
    copyPath: { en: "Copy patch file path", vi: "Chép đường dẫn tệp vá" },
    fields: { en: "Hook fields", vi: "Trường của một hook" },
    fieldsDesc: {
      en: "command, tools, timeout_secs",
      vi: "Ba trường: command, tools, timeout_secs.",
    },
    fieldsMore: {
      en: "command runs through /bin/sh -c. tools is the list of tools it covers, empty meaning every tool. timeout_secs is its own deadline; without it the default is 10 seconds.",
      vi: "command chạy qua /bin/sh -c. tools là danh sách tool nó áp vào, rỗng nghĩa là mọi tool. timeout_secs là hạn giờ riêng, vắng thì lấy mặc định 10 giây.",
    },
    sampleCaption: {
      en: "Sample hook blocking `rm -rf` for bash",
      vi: "Mẫu một hook chặn `rm -rf` cho tool bash",
    },
    copySample: { en: "Copy sample hook config", vi: "Chép mẫu cấu hình hook" },
  },

  // Permissions page.
  perms: {
    scopeRead: common.scopeRead,
    scopeWrite: common.scopeWrite,
    scopeShell: common.scopeShell,

    scopeReadDesc: { en: "Reads and searches the project", vi: "Đọc được tệp và tìm trong dự án, chỉ thế." },
    scopeWriteDesc: {
      en: "Reads and edits project files",
      vi: "Đọc và sửa được tệp trong thư mục dự án.",
    },
    scopeShellDesc: {
      en: "Runs commands on this machine, as you.",
      vi: "Chạy được lệnh trên máy này, dưới quyền của bạn.",
    },

    scopeReadMore: {
      en: "The assistant reads files and searches the project, and nothing more. It edits no file and runs no command.",
      vi: "Trợ lý đọc được tệp và tìm trong dự án, và chỉ thế. Nó không sửa được tệp nào và không chạy được lệnh nào.",
    },
    scopeWriteMore: {
      en: "The assistant reads and edits files inside the project directory. It still runs no command on this machine.",
      vi: "Trợ lý đọc và sửa được tệp trong thư mục dự án. Nó vẫn không chạy được lệnh nào trên máy này.",
    },
    scopeShellMore: {
      en: "The assistant may execute commands on this machine — builds, package installs, deletions — and every command runs under your account, with your rights.",
      vi: "Trợ lý được thi hành lệnh trên máy này — build, cài gói, xoá tệp — và mỗi lệnh chạy dưới đúng tài khoản của bạn, với đúng quyền của bạn.",
    },

    defaultTitle: { en: "Default scope", vi: "Quyền mặc định" },
    defaultDesc: { en: "Where a new turn starts", vi: "Mức mà một lượt mới bắt đầu ở đó." },
    scopeRow: { en: "Tool scope", vi: "Phạm vi tool cho lượt mới" },
    scopeSelect: { en: "Tool scope for new turns", vi: "Phạm vi tool cho lượt mới" },

    shellWarnTitle: {
      en: "This level opens shell commands from the very first turn",
      vi: "Mức này mở lệnh shell ngay từ lượt đầu",
    },
    shellWarnBody: {
      en: "The sandbox blocks neither the network nor reading `~/.ssh`.",
      vi: "Vòng giam không chặn mạng và không chặn đọc `~/.ssh`.",
    },
    shellWarnMore: {
      en: "The sandbox only blocks writes outside the project directory. It blocks neither the network nor reading: a command can still download anything, still read the keys in ~/.ssh, and still send anything out. The only thing left in the way is the approval dialog — it asks before every command, and it tells you the sandbox level on this machine.",
      vi: "Vòng giam chỉ chặn phần ghi ra ngoài thư mục dự án. Nó không chặn mạng và không chặn việc đọc: một lệnh vẫn tải được mọi thứ về, vẫn đọc được khoá nằm trong ~/.ssh, và vẫn gửi được mọi thứ đi. Thứ duy nhất còn đứng chắn là hộp thoại duyệt — nó hỏi trước mỗi lệnh, và nó nói luôn vòng giam trên máy này đang ở mức nào.",
    },

    composerPicker: { en: "Composer picker", vi: "Bộ chọn trong ô soạn tin" },
    composerPickerDesc: { en: "Applies to the open turn only", vi: "Đổi ở đó chỉ áp cho lượt đang mở." },
    composerPickerMore: {
      en: "Still per turn. The setting above only decides which level a new turn opens at; changing it in the composer does not overwrite it.",
      vi: "Vẫn là của từng lượt. Thiết lập ở trên chỉ quyết định lượt mới mở ra ở mức nào; đổi trong ô soạn tin không ghi đè lại nó.",
    },

    sandboxTitle: { en: "Process sandbox", vi: "Vòng giam tiến trình" },
    sandboxDesc: { en: "Read only: a fact, not a setting", vi: "Chỉ đọc: đây là sự thật về máy đang chạy." },
    sandboxMore: {
      en: "Read only. The sandbox is a fact about the machine you are running on, not an option.",
      vi: "Chỉ đọc. Vòng giam là sự thật về máy đang chạy, không phải một tuỳ chọn.",
    },
    levelRow: { en: "Sandbox level", vi: "Mức giam trên máy này" },
    levelDesc: { en: "The kernel enforces it", vi: "Kernel thi hành đúng cái đã khai." },
    levelMore: {
      en: "The kernel enforces exactly what was declared: a write outside the allowed roots fails, not “usually fails”.",
      vi: "Kernel thi hành đúng cái đã khai: ghi ra ngoài vùng cho phép là thất bại, không phải là “thường thì thất bại”.",
    },
    levelUnknown: { en: "core not reachable", vi: "chưa hỏi được lõi" },
    levelFull: { en: "Full", vi: "Đầy đủ" },
    levelPartial: { en: "Partial", vi: "Một phần" },
    levelNone: { en: "None", vi: "Không giam" },

    rootsRow: { en: "Writable roots", vi: "Thư mục ghi được" },
    rootsDesc: { en: "Commands write only here", vi: "Lệnh chỉ ghi được vào những thư mục này." },
    rootsMore: {
      en: "Commands write only here. Everywhere else on disk is read only — but reading is blocked nowhere.",
      vi: "Lệnh chỉ ghi được vào đây. Mọi chỗ khác trên đĩa là chỉ đọc — nhưng đọc thì không bị chặn ở đâu cả.",
    },
    blocksRow: { en: "What it blocks", vi: "Vòng giam chặn gì" },
    blocksDesc: { en: "Only writes outside the project", vi: "Chỉ chặn ghi tệp ngoài thư mục dự án." },
    blocksMore: {
      en: "Only file writes, and only the part outside the project directory. No mode blocks the network, and all three allow reading the whole machine.",
      vi: "Chỉ hiệu ứng ghi lên tệp, và chỉ phần nằm ngoài thư mục dự án. Không chế độ nào chặn mạng, và cả ba chế độ đều cho đọc toàn máy.",
    },
    pluggedRow: { en: "Sandbox plugged in", vi: "Vòng giam đang được cắm" },
    pluggedDesc: { en: "Whether the `sandbox` row exists", vi: "Hàng `sandbox` có trong cây plugin hay không." },
    pluggedMore: {
      en: "Whether the `sandbox` row is in the running plugin tree. Being there is still no guarantee: where the platform has no support it plugs in and reports no sandbox.",
      vi: "Hàng `sandbox` có trong cây plugin đang chạy hay không. Có mặt vẫn chưa chắc giam được: nơi chưa hỗ trợ thì nó cắm rồi tự khai là không giam.",
    },
    pluggedDemo: { en: "no core in the demo", vi: "không có lõi ở bản demo" },
    pluggedAsking: { en: "asking…", vi: "đang hỏi…" },
    pluggedError: { en: "the core did not answer", vi: "không hỏi được lõi" },
    pluggedMissing: { en: "not in the tree", vi: "không có trong cây" },
    pluggedOff: { en: "switched off", vi: "đang bị tắt" },
    pluggedYes: { en: "yes", vi: "có" },

    treeLabel: { en: "Plugin tree", vi: "Cây plugin đang chạy" },
    treeRowOne: { en: "{n} row", vi: "{n} hàng" },
    treeRowMany: { en: "{n} rows", vi: "{n} hàng" },
    treeOff: { en: "[off]", vi: "[tắt]" },
  },

  // Shortcuts page. `keys` are key names and stay untranslated.
  shortcuts: {
    navTitle: common.navigation,
    navDesc: { en: "Work even while typing", vi: "Chạy được kể cả khi đang gõ tin nhắn." },
    findSession: common.findSession,
    findSessionDesc: { en: "Open the session palette", vi: "Mở bảng chọn phiên và lọc theo tên." },
    findSessionMore: {
      en: "Opens the session palette and filters by name, from any screen.",
      vi: "Mở bảng chọn phiên và lọc theo tên, từ bất cứ màn hình nào.",
    },
    closeOpen: { en: "Close what's open", vi: "Đóng thứ đang mở" },
    closeOpenDesc: { en: "Closes dialogs, leaves settings", vi: "Đóng hộp thoại, hoặc thoát khỏi màn hình cài đặt." },

    composerTitle: { en: "Composer", vi: "Soạn tin" },
    composerDesc: { en: "Only inside the composer", vi: "Chỉ có tác dụng trong ô soạn tin." },
    send: common.sendMessage,
    newLine: { en: "New line", vi: "Xuống dòng" },
    newLineDesc: { en: "The composer grows with lines", vi: "Ô soạn tin tự cao thêm theo số dòng." },
    newLineMore: {
      en: "The composer grows with the line count; it has no scrollbar of its own.",
      vi: "Ô soạn tin tự cao thêm theo số dòng, không có thanh cuộn riêng.",
    },
    queue: { en: "Queue while busy", vi: "Xếp hàng khi đang bận" },
    queueDesc: { en: "Sent when this turn ends", vi: "Câu được giữ lại, gửi khi lượt hiện tại xong." },
    queueMore: {
      en: "While the assistant is answering, Enter holds the line and sends it when the current turn ends. Exactly one line waits; typing again replaces it.",
      vi: "Trợ lý đang trả lời thì Enter giữ câu lại và gửi khi lượt hiện tại xong. Đúng một câu chờ; gõ tiếp thì thay câu cũ.",
    },

    completionTitle: { en: "Completions", vi: "Hoàn thành trong ô soạn tin" },
    completionDesc: { en: "The list takes the keys", vi: "Danh sách gợi ý giành phím khi nó đang mở." },
    insertPath: { en: "Insert file path", vi: "Chèn đường dẫn tệp" },
    insertPathDesc: { en: "Type @ then filter", vi: "Gõ @ ở đầu một từ rồi gõ để lọc." },
    insertPathMore: {
      en: "Type @ at the start of a word, then keep typing to filter. Only files the index has scanned show up.",
      vi: "Gõ @ ở đầu một từ rồi gõ tiếp để lọc. Chỉ thấy tệp mà chỉ mục đã quét.",
    },
    commandPalette: { en: "Command palette", vi: "Bảng lệnh" },
    commandPaletteDesc: { en: "Only as the first character", vi: "Chỉ mở khi / là ký tự đầu tiên." },
    commandPaletteMore: {
      en: "Only opens when / is the first character in the field, so typing a path does not pop it up.",
      vi: "Chỉ mở khi / là ký tự đầu tiên của ô nhập, nên gõ một đường dẫn không làm nó bật ra.",
    },
    moveInList: { en: "Move in list", vi: "Đi trong danh sách gợi ý" },
    acceptHit: { en: "Accept highlighted", vi: "Chọn gợi ý đang sáng" },
    closeList: { en: "Close list", vi: "Đóng danh sách gợi ý" },
    closeListDesc: { en: "Keeps what you typed", vi: "Chỉ đóng danh sách, giữ nguyên chữ đã gõ." },
    closeListMore: {
      en: "Only closes the list and keeps what you typed. Keep typing and it opens again.",
      vi: "Chỉ đóng danh sách, giữ nguyên chữ đã gõ. Gõ tiếp thì nó mở lại.",
    },
  },

  // Controls shared by every settings page.
  form: {
    infoDot: { en: "More info", vi: "Giải thích" },
    about: common.about,
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
