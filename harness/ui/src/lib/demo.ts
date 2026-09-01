import type { DisplayMode } from "./prefs";
import type {
  AgentEvent,
  ConversationNode,
  ModelChoice,
  Project,
  SessionSummary,
  ToolScope,
} from "./protocol";

/**
 * Dữ liệu mẫu để `npm run dev` dựng được mọi loại thẻ mà không cần lõi Rust.
 *
 * Bật bằng `?demo=1` trên URL hoặc `VITE_DEMO=1` lúc build. Cờ nằm ngoài đường chạy
 * thật và tệp này không được import ở bất kỳ đâu ngoài `App.tsx`, nên bundler loại nó
 * khỏi bản dựng thật khi cây phụ thuộc không chạm tới — và kể cả khi có chạm, không
 * đường nào từ luồng thật gọi vào đây.
 */
export function isDemo(): boolean {
  if (import.meta.env.VITE_DEMO === "1") return true;
  try {
    return new URLSearchParams(window.location.search).has("demo");
  } catch {
    return false;
  }
}

/**
 * Núm vặn của trang demo: `?demo=1&state=empty&mode=document&changes=1&sidebar=0`.
 *
 * Trang demo tồn tại để *nhìn thấy* từng trạng thái, mà phần lớn trạng thái thú vị lại
 * chỉ xuất hiện trong một khoảnh khắc (khung xương) hoặc chỉ khi dữ liệu vắng mặt (màn
 * hình trống). Không có núm vặn thì cách duy nhất để chụp được chúng là sửa mã.
 */
export function demoKnobs(): {
  state?: "skeleton" | "empty" | "full";
  mode?: DisplayMode;
  changes?: boolean;
  /** Thu thanh bên lại — trạng thái duy nhất mà thanh trên phải tự chừa chỗ cho ba nút
   * giao thông của macOS, và cũng là trạng thái không ai nhớ đi chụp. */
  sidebar?: boolean;
  tab?: string;
  /** Mở sẵn menu dự án — nó chỉ tồn tại trong lúc con trỏ đang giữ nó mở. */
  menu?: string;
  /** Đóng băng trạng thái "đang chuyển dự án", thứ thật ra chỉ kéo dài một nhịp. */
  switching?: boolean;
  /**
   * Dự án nào đang mở: một id, hoặc `"0"` cho **không dự án nào**.
   *
   * `?demo=1&project=0` là trạng thái người dùng gặp **đầu tiên** sau khi cài xong — chưa
   * chọn thư mục nào, trợ lý chỉ trò chuyện. Không có núm này thì nó là trạng thái duy
   * nhất trong ứng dụng chưa ai nhìn thấy bao giờ, vì bộ dữ liệu mẫu luôn mở sẵn một dự án.
   */
  project?: string;
} {
  try {
    const params = new URLSearchParams(window.location.search);
    const state = params.get("state");
    const mode = params.get("mode");
    const changes = params.get("changes");
    const sidebar = params.get("sidebar");
    const tab = params.get("tab");
    const menu = params.get("menu");
    const switching = params.get("switching");
    const project = params.get("project");
    return {
      ...(project === null ? {} : { project }),
      ...(state === "skeleton" || state === "empty" || state === "full" ? { state } : {}),
      ...(mode === "bubble" || mode === "document" ? { mode } : {}),
      ...(changes === null ? {} : { changes: changes !== "0" }),
      ...(sidebar === null ? {} : { sidebar: sidebar !== "0" }),
      ...(tab === null ? {} : { tab }),
      ...(menu === null ? {} : { menu }),
      ...(switching === null ? {} : { switching: switching !== "0" }),
    };
  } catch {
    return {};
  }
}

const MINUTE = 60_000;

/**
 * Mô hình giả cho trang demo.
 *
 * Hai mục ở đây là **trạng thái cần nhìn thấy**, không phải để danh sách dài ra:
 *   - `tools: false` — trạng thái duy nhất mà bộ chọn phải cảnh báo;
 *   - `embedding && !chat` — trạng thái duy nhất mà bộ chọn phải **giấu**. Một dòng bị lọc
 *     mà không có trong bộ mẫu thì không ai kiểm được là nó có bị lọc thật không.
 */
export function demoModels(): ModelChoice[] {
  return [
    { id: "qwen2.5-coder:14b", tools: true, chat: true, embedding: false, contextWindow: 32768 },
    { id: "qwen2.5-coder:32b", tools: true, chat: true, embedding: false, contextWindow: 32768 },
    { id: "gemma3:12b", tools: false, chat: true, embedding: false, contextWindow: 8192 },
    { id: "embeddinggemma:latest", tools: false, chat: false, embedding: true, contextWindow: 2048 },
  ];
}

/**
 * Phiên của **một** dự án.
 *
 * Danh sách phiên bị lọc theo dự án đang mở, nên trang demo phải dựng được đúng điều đó:
 * nếu mọi dự án cùng trả một danh sách thì đổi dự án trông như không có gì xảy ra, và
 * đúng cái không xảy ra đó là thứ cần nhìn thấy.
 */
export function demoSessions(projectId = "p-harness", now = Date.now()): SessionSummary[] {
  // Không dự án nào cũng vẫn có phiên: phía Rust ghi phiên với `cwd` là `null` và coi đó
  // là hợp lệ. Bộ này cố tình chỉ toàn câu hỏi kiến thức — một dòng phụ nhắc tới tệp ở
  // đây sẽ vẽ ra một quá khứ mà trạng thái này không thể có.
  if (projectId === "khong-co-du-an") {
    return [
      {
        id: "kd-1",
        title: "Hỏi nhanh về Rust",
        updatedAt: now - 8 * MINUTE,
        preview: "`async` nhường luồng ở mỗi điểm `await`, còn thread thì do hệ điều hành xếp.",
      },
      { id: "kd-2", title: "Phiên chưa dùng", updatedAt: now - 2 * 24 * 60 * MINUTE, preview: null },
    ];
  }
  if (projectId === "p-notes") {
    return [
      {
        id: "n-1",
        title: "Sắp xếp ghi chú tuần",
        updatedAt: now - 40 * MINUTE,
        preview: "Đã gộp 12 tệp rời vào ba thư mục theo chủ đề.",
      },
    ];
  }
  if (projectId === "p-python") {
    return [
      {
        id: "py-1",
        title: "Gỡ sidecar ASR",
        updatedAt: now - 26 * 60 * MINUTE,
        preview: "Còn hai chỗ import whisper trong download.py.",
      },
      { id: "py-2", title: "Phiên chưa dùng", updatedAt: now - 9 * 24 * 60 * MINUTE, preview: null },
    ];
  }
  return [
    {
      id: "s-diff",
      title: "Sửa bộ nạp cấu hình",
      updatedAt: now - 2 * MINUTE,
      preview: "Có hai chỗ dùng `unwrap` trong `config.rs`. Mình đã thay bằng ConfigError.",
    },
    {
      id: "s-bash",
      title: "Dựng lại chỉ mục",
      updatedAt: now - 95 * MINUTE,
      preview: "Đã dựng lại 9 crate, 1 842 ký hiệu. Mất 12,4 giây.",
    },
    {
      id: "s-read",
      title: "Đọc hợp đồng sự kiện",
      updatedAt: now - 3 * 24 * 60 * MINUTE,
      preview: "Ở app/src/protocol.rs, và bản sao TypeScript ở ui/src/lib/protocol.ts.",
    },
    // Phiên chưa nói gì và cũng chưa mở lần nào: hàng **một dòng**. Nó ở đây để cái đó
    // nhìn thấy được cạnh hàng hai dòng — một trạng thái không dựng ra được thì không ai
    // biết nó trông thế nào cho tới khi gặp trên máy người dùng.
    { id: "s-moi", title: "Phiên chưa dùng", updatedAt: now - 5 * 24 * 60 * MINUTE, preview: null },
  ];
}

const OLD_CONFIG = `fn load(path: &Path) -> Config {
    let raw = fs::read_to_string(path).unwrap();
    serde_norway::from_str(&raw).unwrap()
}`;

const NEW_CONFIG = `fn load(path: &Path) -> Result<Config, ConfigError> {
    let raw = fs::read_to_string(path).map_err(ConfigError::Io)?;
    serde_norway::from_str(&raw).map_err(ConfigError::Parse)
}`;

/** Một bản ghi đủ mọi loại node — dùng để mắt kiểm tra từng thẻ cạnh nhau. */
export function demoNodes(): ConversationNode[] {
  return [
    // Bốn đường đi của bộ dựng khối — mermaid đúng, mermaid sai cú pháp, khối mã ngôn ngữ
    // khác, và một khối chưa đóng rào. Cái nào không có ở đây là cái chưa ai nhìn thấy
    // bao giờ, và ba trong bốn đường đó chỉ hiện ra khi có gì đó không hoàn hảo.
    { id: "d-md-u", kind: "user", text: "Tóm tắt giúp mình luật lọc tool trong pai-tools." },
    { id: "d-md", kind: "assistant", text: demoMarkdownText(), streaming: false },
    { id: "d-u0", kind: "user", text: "Vẽ giúp mình kiến trúc của cây plugin." },
    { id: "d-diagram", kind: "assistant", text: demoDiagramText(), streaming: false },
    { id: "d-u1", kind: "user", text: "Bỏ hết unwrap trong bộ nạp cấu hình giúp mình." },
    {
      id: "d-todo",
      kind: "todo",
      items: [
        { id: "1", text: "Tìm chỗ dùng unwrap", status: "done" },
        { id: "2", text: "Thêm kiểu lỗi ConfigError", status: "in_progress" },
        { id: "3", text: "Chạy lại bộ test", status: "pending" },
        { id: "4", text: "Cập nhật tài liệu", status: "cancelled" },
      ],
    },
    { id: "d-notice", kind: "notice", message: "Đã ghim lượt này vào workspace harness/." },
    { id: "d-progress", kind: "progress", label: "Đang nạp mô hình", detail: "qwen2.5-coder:14b" },
    {
      id: "d-t1",
      kind: "tool",
      call: {
        callId: "c1",
        name: "grep",
        args: { pattern: "unwrap\\(\\)", path: "crates/" },
        state: "ok",
        meta: {
          search: {
            shape: "matches",
            truncated: true,
            total: 42,
            groups: [
              {
                path: "crates/pai-core/src/config.rs",
                matches: [
                  { line: 12, text: "    let raw = fs::read_to_string(path).unwrap();" },
                  { line: 13, text: "    serde_norway::from_str(&raw).unwrap()" },
                ],
              },
              {
                path: "crates/pai-tools/src/registry.rs",
                matches: [
                  { line: 88, text: "        self.by_name.get(name).unwrap()" },
                  { line: 91, text: "        scope.parse().unwrap()" },
                  { line: 97, text: "        entry.schema().unwrap()" },
                  { line: 104, text: "        guard.lock().unwrap()" },
                ],
              },
            ],
          },
        },
      },
    },
    {
      id: "d-t2",
      kind: "tool",
      call: {
        callId: "c2",
        name: "glob",
        args: { pattern: "crates/**/config*.rs" },
        state: "ok",
        meta: {
          search: {
            shape: "paths",
            truncated: false,
            total: 3,
            paths: [
              "crates/pai-core/src/config.rs",
              "crates/pai-agent/src/config_bridge.rs",
              "crates/pai-tools/src/config_scope.rs",
            ],
          },
        },
      },
    },
    {
      id: "d-t3",
      kind: "tool",
      call: {
        callId: "c3",
        name: "read",
        args: { file_path: "crates/pai-core/src/config.rs" },
        state: "ok",
        meta: {
          read: {
            path: "crates/pai-core/src/config.rs",
            offset: 10,
            total_lines: 214,
            lang: "rust",
            lines: OLD_CONFIG.split("\n").map((text, index) => ({ number: 10 + index, text })),
          },
        },
      },
    },
    {
      id: "d-a1",
      kind: "assistant",
      text: "Có hai chỗ `unwrap` trong `config.rs`. Mình đổi hàm sang trả `Result` và bọc lỗi lại.",
      streaming: false,
    },
    {
      id: "d-t4",
      kind: "tool",
      call: {
        callId: "c4",
        name: "edit",
        args: {
          file_path: "crates/pai-core/src/config.rs",
          old_string: OLD_CONFIG,
          new_string: NEW_CONFIG,
        },
        state: "ok",
        meta: {
          diffs: [
            {
              path: "crates/pai-core/src/config.rs",
              old_text: OLD_CONFIG,
              new_text: NEW_CONFIG,
              old_start: 10,
              new_start: 10,
            },
          ],
        },
      },
    },
    {
      id: "d-t5",
      kind: "tool",
      call: {
        callId: "c5",
        name: "write",
        args: { file_path: "crates/pai-core/src/error.rs", content: "pub enum ConfigError {}\n" },
        state: "running",
        intendedDiffs: [
          {
            path: "crates/pai-core/src/error.rs",
            old_text: null,
            new_text:
              "use std::io;\n\n#[derive(Debug)]\npub enum ConfigError {\n    Io(io::Error),\n    Parse(serde_norway::Error),\n}\n",
          },
        ],
      },
    },
    {
      id: "d-t6",
      kind: "tool",
      call: {
        callId: "c6",
        name: "bash",
        args: { command: "cargo test -p pai-core" },
        state: "ok",
        meta: {
          terminal: {
            command: "cargo test -p pai-core",
            cwd: "/Users/vinhpx/Workspaces/private-ai/harness",
            exit_code: 101,
            output: [
              "   Compiling pai-core v0.1.0",
              "    Finished test profile in 8.42s",
              "     Running unittests src/lib.rs",
              "",
              "running 24 tests",
              "test config::tests::rejects_missing_file ... FAILED",
              "test config::tests::parses_minimal ... ok",
              "test event::tests::waterfall_stops_on_deny ... ok",
              "test event::tests::notify_is_fire_and_forget ... ok",
              "test service::tests::wait_for_resolves ... ok",
              "",
              "failures:",
              "    config::tests::rejects_missing_file",
              "",
              "test result: FAILED. 23 passed; 1 failed",
            ].join("\n"),
          },
        },
      },
    },
    {
      id: "d-t7",
      kind: "tool",
      call: {
        callId: "c7",
        name: "bash",
        args: { command: "cargo watch -x check" },
        state: "running",
        meta: {
          terminal: {
            command: "cargo watch -x check",
            output: "[watching for changes]",
            exit_code: null,
            background: true,
            job_id: "job-3",
          },
        },
      },
    },
    {
      id: "d-t8",
      kind: "tool",
      call: {
        callId: "c8",
        name: "mcp__jira__jira_get_issue",
        args: { issue_key: "PAI-142", fields: "summary,status" },
        state: "ok",
        preview: '{"key":"PAI-142","status":"In Progress"}',
      },
    },
    { id: "d-err", kind: "error", message: "Mô hình ngắt kết nối giữa chừng (ollama: connection reset)." },
  ];
}

/**
 * Một lượt giả, phát đúng hình dạng sự kiện của lõi — kể cả câu hỏi duyệt.
 *
 * `settleApproval` để chỗ gọi chặn lượt lại cho tới khi người dùng bấm. Không có nó thì
 * hộp thoại duyệt hiện lên rồi biến mất sau nửa giây, tức là đúng thứ *không* xảy ra
 * thật — và trang demo tồn tại để nhìn thấy thứ xảy ra thật.
 */
export async function runDemoTurn(
  text: string,
  scope: ToolScope,
  onEvent: (event: AgentEvent) => void,
  settleApproval: () => Promise<void> = async () => {},
): Promise<void> {
  const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

  onEvent({ kind: "notice", message: "Chế độ demo: không có lõi nào chạy thật." });
  for (const word of `Mình sẽ xem qua "${text}" rồi sửa. `.split(" ")) {
    onEvent({ kind: "token", text: `${word} ` });
    await wait(40);
  }

  onEvent({ kind: "todo", items: [
    { id: "1", text: "Đọc tệp liên quan", status: "in_progress" },
    { id: "2", text: "Áp thay đổi", status: "pending" },
  ] });

  onEvent({ kind: "tool_start", call_id: "demo-1", name: "read", args: { file_path: "crates/pai-core/src/config.rs" } });
  await wait(350);
  onEvent({
    kind: "tool_end",
    call_id: "demo-1",
    name: "read",
    is_error: false,
    preview: "…",
    meta: {
      read: {
        path: "crates/pai-core/src/config.rs",
        offset: 10,
        total_lines: 214,
        lines: OLD_CONFIG.split("\n").map((line, index) => ({ number: 10 + index, text: line })),
      },
    },
  });

  // Phạm vi chỉ đọc thì lõi thật **không quảng cáo** `edit` nữa, nên bản demo cũng không
  // được diễn cảnh sửa tệp: một trang demo hứa nhiều hơn sản phẩm là cùng một lời nói dối
  // mà bộ chọn phạm vi sinh ra để chấm dứt.
  if (scope === "read") {
    onEvent({ kind: "todo", items: [
      { id: "1", text: "Đọc tệp liên quan", status: "done" },
      { id: "2", text: "Áp thay đổi", status: "cancelled" },
    ] });
    for (const word of "Lượt này ở phạm vi chỉ đọc nên mình chưa sửa được tệp. Nâng lên \"Đọc và ghi\" rồi nhắn lại là mình áp thay đổi.".split(" ")) {
      onEvent({ kind: "token", text: `${word} ` });
      await wait(40);
    }
    onEvent({ kind: "final", message_id: "demo" });
    return;
  }

  onEvent({ kind: "tool_start", call_id: "demo-2", name: "edit", args: { file_path: "crates/pai-core/src/config.rs", old_string: OLD_CONFIG, new_string: NEW_CONFIG } });
  onEvent({
    kind: "approval_request",
    request_id: "demo-approval",
    call_id: "demo-2",
    name: "edit",
    args: { file_path: "crates/pai-core/src/config.rs", old_string: OLD_CONFIG, new_string: NEW_CONFIG },
    reason: "Tệp nằm ngoài vùng đã được cấp quyền ghi.",
    timeout_ms: 30_000,
  });
  await settleApproval();
  await wait(250);
  onEvent({
    kind: "tool_end",
    call_id: "demo-2",
    name: "edit",
    is_error: false,
    preview: "đã ghi 4 dòng",
    meta: {
      diffs: [{ path: "crates/pai-core/src/config.rs", old_text: OLD_CONFIG, new_text: NEW_CONFIG, old_start: 10, new_start: 10 }],
    },
  });

  onEvent({ kind: "todo", items: [
    { id: "1", text: "Đọc tệp liên quan", status: "done" },
    { id: "2", text: "Áp thay đổi", status: "done" },
  ] });
  for (const word of "Xong. Hàm giờ trả Result thay vì panic.".split(" ")) {
    onEvent({ kind: "token", text: `${word} ` });
    await wait(40);
  }
  onEvent({ kind: "final", message_id: "demo" });
}

/**
 * Bản ghi rút gọn của những phiên **không** đang mở.
 *
 * Danh sách phiên lấy dòng phụ từ bản ghi đã nạp, nên không có mấy cái này thì trong
 * trang demo chỉ đúng một hàng có dòng phụ — và ta sẽ không nhìn thấy hàng ba dòng trông
 * ra sao khi cả cột đều đầy.
 */
export function demoParked(): Record<string, ConversationNode[]> {
  return {
    "s-bash": [
      { id: "p1-u", kind: "user", text: "Dựng lại chỉ mục tree-sitter cho crates/." },
      {
        id: "p1-a",
        kind: "assistant",
        text: "Đã dựng lại 9 crate, 1 842 ký hiệu. Mất 12,4 giây.",
        streaming: false,
      },
    ],
    "s-read": [
      { id: "p2-u", kind: "user", text: "Hợp đồng sự kiện giữa Rust và giao diện nằm ở đâu?" },
      {
        id: "p2-a",
        kind: "assistant",
        text: "Ở app/src/protocol.rs, và bản sao TypeScript ở ui/src/lib/protocol.ts.",
        streaming: false,
      },
    ],
  };
}

/**
 * Dự án giả.
 *
 * Ba cái, và ba cái là số nhỏ nhất còn nhìn thấy được thứ tự "mới nhất trước" — với hai
 * cái thì mọi thứ tự đều đúng, và một menu sắp sai sẽ đi qua mà không ai nhận ra.
 *
 * `current` chọn dòng nào đang mở. `"0"` — hay bất kỳ id nào không có trong danh sách —
 * nghĩa là **không dự án nào**: danh sách vẫn đủ ba dòng, đúng như sau một lần "Đóng dự
 * án", vì đóng không bỏ dòng nào khỏi danh sách.
 */
export function demoProjects(current = "p-harness", now = Date.now()): Project[] {
  const all: Project[] = [
    {
      id: "p-harness",
      name: "harness",
      path: "/Users/vinhpx/Workspaces/private-ai/harness",
      lastOpenedAt: now - 3 * MINUTE,
      isCurrent: false,
      kind: "code",
      origin: null,
    },
    {
      id: "p-python",
      name: "private-ai",
      path: "/Users/vinhpx/Workspaces/private-ai",
      lastOpenedAt: now - 22 * 60 * MINUTE,
      isCurrent: false,
      kind: "code",
      // Dự án duy nhất trong bộ mẫu được clone về: nhãn nguồn gốc phải có chỗ để lộ ra.
      origin: "https://github.com/vinhpx/private-ai.git",
    },
    {
      id: "p-notes",
      name: "so-tay",
      path: "/Users/vinhpx/Documents/so-tay",
      lastOpenedAt: now - 6 * 24 * 60 * MINUTE,
      // Một dự án tài liệu trong danh sách, nếu không thì nhánh `docs` của mọi màn hình
      // không bao giờ được nhìn thấy ở chế độ trình diễn.
      kind: "docs",
      origin: null,
      isCurrent: false,
    },
  ];
  return all.map((entry) => ({ ...entry, isCurrent: entry.id === current }));
}

/**
 * Một tin nhắn trợ lý đi qua **mọi** loại token markdown mà `Markdown.tsx` dựng được.
 *
 * Danh sách này là danh sách kiểm, không phải một câu trả lời trông cho giống thật: đậm,
 * nghiêng, mã inline, gạch ngang, tiêu đề, danh sách có số và không số **lồng nhau**, danh
 * sách việc, bảng có căn cột, trích dẫn, đường kẻ ngang, liên kết. Cái gì không có ở đây là
 * cái chưa ai nhìn thấy bao giờ — và với một bộ dựng token thì chỗ hỏng luôn là đúng một
 * loại token không ai nghĩ tới.
 *
 * Cố ý **không có khối rào** trong này: đường rào đã có `demoDiagramText` lo, và tách hai
 * bộ mẫu ra thì lúc một trong hai hỏng, nhìn là biết hỏng ở bên nào.
 */
function demoMarkdownText(): string {
  return [
    "## Lọc tool hai tầng",
    "",
    "Sổ đăng ký kiểm quyền ở **hai chỗ**, và bỏ một chỗ là mở một đường vòng:",
    "",
    "1. Lúc *liệt kê* — mô hình không thấy tên tool ngoài phạm vi.",
    "2. Lúc *gọi*, sau khi đã gỡ tên wire:",
    "   - tên lạ rơi vào `Deny`;",
    "   - tham số `workspace` bị ghi đè, không nhận mặc định.",
    "",
    "| Tầng | Hàm | Bỏ qua được? |",
    "| --- | :--- | ---: |",
    "| Liệt kê | `Registry::visible` | không |",
    "| Gọi | `Registry::invoke` | không |",
    "| ~~Hook~~ | `on_pre_call` | có (fail-open) |",
    "",
    "### Việc còn lại",
    "",
    "- [x] Gỡ nhánh `Allow` khỏi guard",
    "- [ ] Viết test cho tên gọi thẳng",
    "",
    "> Guard đơn điệu: chỉ `Deny` hoặc bỏ phiếu trắng. Có `Allow` thì thứ tự đăng ký biến",
    "> một lần từ chối thành một lần cho phép.",
    "",
    "---",
    "",
    "Chi tiết ở [CONTRACT.md](https://example.invalid/CONTRACT.md), mục *Ranh giới tin cậy*.",
  ].join("\n");
}

/**
 * Một tin nhắn trợ lý đi qua **cả bốn** đường của bộ dựng khối.
 *
 * Khối mermaid chưa đóng rào nằm ở cuối, đúng chỗ nó xuất hiện lúc chữ đang chảy. Không
 * có nó trong dữ liệu mẫu thì đường đi quan trọng nhất của `Blocks.tsx` — đường không
 * được gọi `mermaid.render` — là đường chưa ai nhìn thấy bao giờ.
 */
function demoDiagramText(): string {
  return [
    "Đường đi của một lượt gọi provider, từ lúc agent đẩy lượt vào hàng đợi:",
    "",
    "```mermaid",
    "flowchart LR",
    '  q(["Vec#60;PendingTurn#62;"]) --> d{{"Driver"}}',
    '  d --> s(["OpenAiDriver::stream_turn"])',
    '  s -->|gọi| r(["retry_with_backoff"])',
    '  r -.->|tham chiếu| p["RetryPolicy#60;Backoff#62;"]',
    "```",
    "",
    "Chính sách thử lại đọc từ tệp cấu hình:",
    "",
    "```rust",
    "let policy = RetryPolicy::<Backoff>::from_env()",
    '    .with_predicate(Box::new(|code: &StatusCode| code.as_u16() == 429));',
    "```",
    "",
    "Còn đây là một sơ đồ tôi viết sai cú pháp — mermaid sẽ từ chối nó:",
    "",
    "```mermaid",
    "flowchart LR",
    "  a --> ",
    "  --> b]]",
    "```",
    "",
    "Và phần tôi đang vẽ dở:",
    "",
    "```mermaid",
    "sequenceDiagram",
    "  participant A as Agent",
    "  A->>D: stream_turn",
  ].join("\n");
}
