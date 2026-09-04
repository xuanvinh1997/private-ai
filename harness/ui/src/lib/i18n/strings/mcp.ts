import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `mcp` area. See lib/i18n/README.md. Three groups deliberately ignore the "at most 5 words"
 * rule: the trust wording, the irreversible remove dialog, and the error messages. */
export const mcp = {
  // Main page
  title: { en: "Servers", vi: "Server đang cắm" },
  desc: { en: "External tools: repos, databases", vi: "Cắm công cụ ngoài: kho mã, cơ sở dữ liệu." },
  add: { en: "Add server", vi: "Thêm server" },
  reloading: { en: "Reloading…", vi: "Đang nạp lại…" },
  empty: { en: "No servers yet", vi: "Chưa cắm server nào — MCP thêm tool ngoài dự án." },
  openCatalog: { en: "Browse catalog", vi: "Mở danh mục" },

  // Server states, lower case: these are inline labels, not titles.
  state: {
    connected: { en: "connected", vi: "đã nối" },
    connecting: { en: "connecting", vi: "đang nối" },
    failed: { en: "failed", vi: "hỏng" },
    disabled: { en: "off", vi: "đang tắt" },
  },

  // The core's policy, pinned to the top of the page and not collapsible.
  trust: {
    title: {
      en: "MCP servers return untrusted content",
      vi: "Server MCP trả về nội dung không đáng tin",
    },
    body: { en: "Only add servers you trust.", vi: "Chỉ cắm server bạn tin." },
    more: {
      en: "Everything an MCP server returns is framed by the core as outside data, and every one of its tools is treated as able to change state — so they all go through the approval step, even when the tool name sounds read-only. Plugging in a server lets its author speak into your conversation; plug in only what you trust.",
      vi: "Mọi thứ một server MCP trả về đều được lõi đóng khung là dữ liệu bên ngoài, và mọi tool của nó luôn bị coi là có thể thay đổi trạng thái — nên chúng đi qua bước hỏi duyệt, kể cả khi tên tool nghe như chỉ đọc. Cắm một server là cho tác giả của nó nói vào cuộc hội thoại của bạn; chỉ cắm cái bạn tin.",
    },
  },

  // Tool list of one row
  toolsOne: { en: "{n} tool", vi: "{n} tool" },
  toolsMany: { en: "{n} tools", vi: "{n} tool" },
  showToolsOne: { en: "Show {n} tool", vi: "Xem {n} tool đã cắm" },
  showToolsMany: { en: "Show {n} tools", vi: "Xem {n} tool đã cắm" },
  hideTools: { en: "Hide tools", vi: "Ẩn danh sách tool" },
  toolNames: {
    en: "Names the model sees",
    vi: "Đây là tên mô hình thấy, và tên trong bản ghi.",
  },
  noTools: { en: "Connected, no tools", vi: "Nối được nhưng không có tool nào." },
  noToolsLabel: { en: "Why this server has no tools", vi: "Vì sao server không có tool" },
  noToolsMore: {
    en: "The server connected but declared no tools, so it adds nothing for the assistant. Check the command-line arguments, or the permissions on the token.",
    vi: "Server nối được nhưng không khai báo tool nào, nên nó chưa thêm gì cho trợ lý. Kiểm tra lại tham số dòng lệnh hoặc quyền của token.",
  },

  // Accessible labels for the repeated row buttons; not shortened, these name the control.
  turnOn: { en: "Turn on server {name}", vi: "Bật server {name}" },
  turnOff: { en: "Turn off server {name}", vi: "Tắt server {name}" },
  editServer: { en: "Edit server {name}", vi: "Sửa server {name}" },
  deleteServer: { en: "Delete server {name}", vi: "Xoá server {name}" },

  remove: {
    title: { en: "Delete server {name}?", vi: "Xoá server {name}?" },
    body: {
      en: "Removes the config, the environment variables and {n} tools.",
      vi: "Xoá hẳn cấu hình, biến môi trường và {n} tool.",
    },
    more: {
      en: "Its config and every environment variable are deleted from this machine, and {n} tools disappear from the assistant. This cannot be undone.",
      vi: "Cấu hình và mọi biến môi trường của nó bị xoá khỏi máy, và {n} tool biến mất khỏi trợ lý. Không hoàn tác được.",
    },
    confirm: { en: "Delete server", vi: "Xoá server" },
  },

  errors: {
    actionTitle: { en: "Action failed", vi: "Không làm được" },
    toggle: {
      en: "Could not change the server state: {msg}",
      vi: "Không đổi được trạng thái: {msg}",
    },
    remove: { en: "Could not delete the server: {msg}", vi: "Không xoá được server: {msg}" },
    reload: { en: "Could not reload: {msg}", vi: "Không nạp lại được: {msg}" },
    saveTitle: { en: "Not saved", vi: "Không lưu được" },
    installTitle: { en: "Not plugged in", vi: "Không cắm được" },
  },

  form: {
    addTitle: { en: "Add server", vi: "Thêm server MCP" },
    editTitle: common.editName,
    desc: { en: "Tools appear as ext.<name>.<tool>.", vi: "Tool hiện dưới tên ext.<tên>.<tool>." },
    submit: { en: "Plug in", vi: "Cắm server" },

    pasteSummary: { en: "Paste JSON", vi: "Dán JSON từ tài liệu của server" },
    jsonLabel: { en: "mcpServers block", vi: "Khối mcpServers" },
    jsonFill: { en: "Fill fields", vi: "Điền vào các ô" },
    jsonBadTitle: { en: "Bad JSON", vi: "JSON không dùng được" },
    jsonFilled: {
      en: 'Filled from "{name}" — review it, then save.',
      vi: 'Đã điền từ mục "{name}" — xem lại rồi bấm Lưu.',
    },
    jsonFilledRest: {
      en: 'Filled "{name}", skipped the other {n} entries ({list}).',
      vi: 'Đã điền "{name}", bỏ qua {n} mục còn lại ({list}).',
    },

    name: common.name,
    nameHint: {
      en: "Prefix for every tool; lowercase, no spaces",
      vi: "Tiền tố của mọi tool; chữ thường, không dấu cách.",
    },
    nameLocked: { en: "The name is the identifier", vi: "Tên là khoá định danh nên không sửa được." },
    nameLockedMore: {
      en: "The name is the server's identifier, so it cannot be edited — delete it and add it again to change it.",
      vi: "Tên là khoá định danh của server nên không sửa được — xoá rồi thêm lại nếu cần đổi.",
    },

    transport: { en: "Connection", vi: "Cách kết nối" },
    stdio: { en: "Child process (stdio)", vi: "Tiến trình con (stdio)" },
    transportHint: {
      en: "stdio runs a command on this machine; HTTP calls a URL.",
      vi: "stdio chạy lệnh tại máy; HTTP gọi một địa chỉ.",
    },

    command: { en: "Command", vi: "Lệnh" },
    args: { en: "Arguments", vi: "Tham số" },
    argsHint: { en: "One argument per line, in order", vi: "Mỗi dòng một tham số, đúng thứ tự." },
    argsMore: {
      en: "Don't fold a whole command line into one field — a space inside an argument is a real space.",
      vi: "Đừng gộp cả dòng lệnh vào một ô — dấu cách trong một tham số là dấu cách thật.",
    },
    argsAdd: { en: "Add argument", vi: "Thêm tham số" },

    env: { en: "Environment variables", vi: "Biến môi trường" },
    envHint: {
      en: "Key and value — where the token goes",
      vi: "Khoá và giá trị — chỗ đặt token của server.",
    },
    envAdd: { en: "Add variable", vi: "Thêm biến" },

    cwd: { en: "Working directory", vi: "Thư mục làm việc" },

    headers: { en: "Headers", vi: "Header" },
    headersHint: { en: "Example Authorization: Bearer …", vi: "Ví dụ Authorization: Bearer …" },
    headersAdd: { en: "Add header", vi: "Thêm header" },

    enable: { en: "Turn on after saving", vi: "Bật ngay sau khi lưu" },
    enableHint: {
      en: "Off means no tools reach the model.",
      vi: "Tắt thì tool không đến tay mô hình.",
    },

    rowKey: { en: "{field} — key", vi: "{field} — khoá" },
    rowValue: { en: "{field} — value", vi: "{field} — giá trị" },
    rowKeyPlaceholder: { en: "KEY", vi: "KHOA" },
    rowRemove: { en: "Remove this row from {field}", vi: "Bỏ dòng này khỏi {field}" },
    rowAbout: { en: "About {field}", vi: "Về {field}" },
  },

  catalog: {
    title: { en: "Catalog", vi: "Danh mục server MCP" },
    installTitle: { en: "Plug in {name}", vi: "Cắm {name}" },
    desc: {
      en: "Each adds tools under ext.<server>.",
      vi: "Mỗi mục thêm một bộ tool có tiền tố ext.<server>.",
    },
    descPicked: { en: "Fill what the server needs", vi: "Điền các biến server cần, rồi cắm." },
    secretMore: {
      en: "Secret values go straight into the core and are never shown again. Once it is plugged in the dialog closes, and there is no way to read one back onto the screen.",
      vi: "Giá trị bí mật đi thẳng vào lõi và không hiện lại. Sau khi cắm, hộp thoại đóng và không có đường nào đọc ngược ra màn hình.",
    },
    manual: { en: "Declare manually…", vi: "Tự khai báo…" },
    back: { en: "Back to catalog", vi: "Quay lại danh mục" },
    submit: { en: "Plug in", vi: "Cắm server" },
    empty: {
      en: "No presets — declare one below.",
      vi: "Chưa có mục dựng sẵn nào; tự khai báo bên dưới.",
    },
    remote: { en: "Runs remotely — nothing to install", vi: "Chạy từ xa — không cần cài gì" },
    needsOne: { en: "{n} variable needed", vi: "Cần điền {n} biến" },
    needsMany: { en: "{n} variables needed", vi: "Cần điền {n} biến" },
    requiresTitle: { en: "Requires", vi: "Máy này phải có sẵn" },
    requiresBody: { en: "Missing one and it fails", vi: "Thiếu một thứ thì server cắm hỏng." },
    requiresMore: {
      en: "If one of these is missing the server fails to plug in, and the error is a line from the operating system rather than a sentence written for you.",
      vi: "Thiếu một trong số này thì server sẽ cắm hỏng, và thông điệp lỗi là một dòng của hệ điều hành chứ không phải một câu tiếng Việt.",
    },
    noEnv: {
      en: "This server needs no environment variables.",
      vi: "Server này không cần biến môi trường nào.",
    },
    optional: { en: "— optional", vi: "— tuỳ chọn" },
    required: { en: "— required", vi: "— bắt buộc" },
    secretNote: { en: "never shown again after plugging in", vi: "không hiện lại sau khi cắm" },
    missing: {
      en: "Still missing: *{list}* — fill these in and the button lights up.",
      vi: "Còn thiếu: *{list}* — điền xong thì nút sáng lên.",
    },
  },

  /** Human-readable names of the things that must already be on the machine. */
  requires: {
    node: { en: "Node.js (the npx command)", vi: "Node.js (lệnh npx)" },
    python: { en: "Python (the uvx or pipx command)", vi: "Python (lệnh uvx hoặc pipx)" },
    docker: { en: "Docker running", vi: "Docker đang chạy" },
  },

  /** Errors from parsing a pasted JSON block; they say what broke and what to do next. */
  json: {
    empty: { en: "Nothing pasted into the JSON field.", vi: "Chưa dán gì vào ô JSON." },
    unreadable: { en: "JSON could not be read: {msg}", vi: "JSON không đọc được: {msg}" },
    notObject: {
      en: 'JSON must be an object, for example {"mcpServers": { … }}.',
      vi: 'JSON phải là một đối tượng, ví dụ {"mcpServers": { … }}.',
    },
    noEntries: {
      en: 'No entry found under "mcpServers". Paste the whole block, outer braces included.',
      vi: 'Không thấy mục nào trong "mcpServers". Dán cả khối, kể cả dấu ngoặc ngoài.',
    },
    entryNotObject: {
      en: 'Entry "{name}" is not an object.',
      vi: 'Mục "{name}" không phải một đối tượng.',
    },
    entryNoTarget: {
      en: 'Entry "{name}" has neither "command" nor "url" — nothing to run.',
      vi: 'Mục "{name}" không có "command" lẫn "url" — không biết chạy cái gì.',
    },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
