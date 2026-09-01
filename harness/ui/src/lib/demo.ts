import { demoDiagramText } from "./fixtures/graph";
import type { DisplayMode } from "./prefs";
import type {
  AgentEvent,
  ConversationNode,
  FileView,
  ModelChoice,
  Project,
  SessionSummary,
  TreeEntry,
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
 * Núm vặn của trang demo: `?demo=1&state=empty&mode=document&changes=1`.
 *
 * Trang demo tồn tại để *nhìn thấy* từng trạng thái, mà phần lớn trạng thái thú vị lại
 * chỉ xuất hiện trong một khoảnh khắc (khung xương) hoặc chỉ khi dữ liệu vắng mặt (màn
 * hình trống). Không có núm vặn thì cách duy nhất để chụp được chúng là sửa mã.
 */
export function demoKnobs(): {
  state?: "skeleton" | "empty" | "full";
  mode?: DisplayMode;
  changes?: boolean;
  tab?: string;
  /** Mở sẵn menu dự án — nó chỉ tồn tại trong lúc con trỏ đang giữ nó mở. */
  menu?: string;
  /** Đóng băng trạng thái "đang chuyển dự án", thứ thật ra chỉ kéo dài một nhịp. */
  switching?: boolean;
  /** Mở sẵn một tệp trong tab Mã nguồn, để chụp khung xem mà không phải bấm. */
  file?: string;
} {
  try {
    const params = new URLSearchParams(window.location.search);
    const state = params.get("state");
    const mode = params.get("mode");
    const changes = params.get("changes");
    const tab = params.get("tab");
    const menu = params.get("menu");
    const switching = params.get("switching");
    const file = params.get("file");
    return {
      ...(state === "skeleton" || state === "empty" || state === "full" ? { state } : {}),
      ...(mode === "bubble" || mode === "document" ? { mode } : {}),
      ...(changes === null ? {} : { changes: changes !== "0" }),
      ...(tab === null ? {} : { tab }),
      ...(menu === null ? {} : { menu }),
      ...(switching === null ? {} : { switching: switching !== "0" }),
      ...(file === null ? {} : { file }),
    };
  } catch {
    return {};
  }
}

const MINUTE = 60_000;

/**
 * Mô hình giả cho trang demo.
 *
 * Có một mô hình `tools: false` là cố ý: nó là trạng thái duy nhất mà giao diện phải
 * cảnh báo, nên nó phải nhìn thấy được mà không cần dựng máy chủ.
 */
export function demoModels(): ModelChoice[] {
  return [
    { id: "qwen2.5-coder:14b", tools: true, contextWindow: 32768 },
    { id: "qwen2.5-coder:32b", tools: true, contextWindow: 32768 },
    { id: "gemma3:12b", tools: false, contextWindow: 8192 },
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
 */
export function demoProjects(now = Date.now()): Project[] {
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
}

interface Draft {
  name: string;
  children?: Draft[];
}

const dir = (name: string, ...children: Draft[]): Draft => ({ name, children });
const file = (name: string): Draft => ({ name });

/**
 * Cây tệp giả, dựng **đầy đủ một lần** rồi cắt theo `depth` lúc trả.
 *
 * Ngược đời so với phía thật, nơi dữ liệu đắt nên phải nạp lười. Ở đây dữ liệu là hằng
 * số, còn thứ cần dựng lại đúng là *hình dạng câu trả lời*: cắt theo `depth` để cây thật
 * và cây giả cùng thiếu `children` ở đúng những chỗ giống nhau. Không cắt thì cây trong
 * demo tự mở hết và bug nạp lười sẽ không bao giờ lộ ra ở đây.
 */
const TREES: Record<string, Draft[]> = {
  "p-harness": [
    dir(
      "app",
      dir("src", file("approval.rs"), file("coalesce.rs"), file("harness.rs"), file("lib.rs"), file("main.rs"), file("protocol.rs")),
      file("Cargo.toml"),
    ),
    dir(
      "crates",
      dir("pai-core", dir("src", file("config.rs"), file("event.rs"), file("lib.rs"), file("service.rs"))),
      dir("pai-agent", dir("src", file("lib.rs"), file("turn.rs"))),
      dir("pai-tools", dir("src", file("lib.rs"), file("registry.rs"))),
    ),
    dir("docs", file("ARCHITECTURE.md"), file("PACKAGING.md"), file("ROADMAP.md")),
    dir(
      "ui",
      dir(
        "src",
        dir("components", file("Rail.tsx"), file("SessionPanel.tsx"), file("Transcript.tsx")),
        dir("lib", file("demo.ts"), file("protocol.ts"), file("registry.ts")),
        dir("styles", file("app.css"), file("tokens.css")),
        file("App.tsx"),
        file("index.tsx"),
      ),
      file("package.json"),
      file("vite.config.ts"),
    ),
    file("Cargo.lock"),
    file("Cargo.toml"),
    file("README.md"),
  ],
  "p-python": [
    dir("src", dir("private_ai", dir("asr", file("download.py")), dir("ui", file("theme.py")))),
    dir("tests", file("test_asr_packaging.py")),
    file("pyproject.toml"),
    file("README.md"),
  ],
  "p-notes": [dir("2026", file("thang-08.md"), file("thang-09.md")), file("index.md")],
};

function build(drafts: Draft[], prefix: string): TreeEntry[] {
  return drafts.map((draft) => {
    const path = prefix === "" ? draft.name : `${prefix}/${draft.name}`;
    return draft.children === undefined
      ? { path, name: draft.name, isDir: false }
      : { path, name: draft.name, isDir: true, children: build(draft.children, path) };
  });
}

function prune(entries: TreeEntry[], depth: number): TreeEntry[] {
  return entries.map((entry) => {
    if (!entry.isDir) return { path: entry.path, name: entry.name, isDir: false };
    if (depth <= 1) return { path: entry.path, name: entry.name, isDir: true };
    return {
      path: entry.path,
      name: entry.name,
      isDir: true,
      children: prune(entry.children ?? [], depth - 1),
    };
  });
}

function findIn(entries: TreeEntry[], path: string): TreeEntry | undefined {
  for (const entry of entries) {
    if (entry.path === path) return entry;
    if (entry.isDir && path.startsWith(`${entry.path}/`)) {
      const hit = findIn(entry.children ?? [], path);
      if (hit) return hit;
    }
  }
  return undefined;
}

/**
 * Gốc trên đĩa của một dự án giả.
 *
 * Cây giả mang đường dẫn **tuyệt đối**, đúng như `list_tree` thật: nó chuẩn hoá đường dẫn
 * để chứng minh tệp nằm trong dự án, nên thứ nó trả ra không bao giờ là đường dẫn tương
 * đối. Cho demo trả đường dẫn tương đối sẽ dựng ra một giao diện chạy đẹp ở đây và gãy
 * ở lần đầu chạy thật.
 */
export function demoRoot(projectId: string): string {
  return demoProjects().find((entry) => entry.id === projectId)?.path ?? "";
}

/** Một cấp của cây giả. Trễ nhân tạo để trạng thái "đang nạp" nhìn thấy được. */
export async function demoTree(
  projectId: string,
  path?: string,
  depth = 1,
): Promise<TreeEntry[]> {
  await new Promise<void>((resolve) => setTimeout(resolve, 180));
  const roots = build(TREES[projectId] ?? [], demoRoot(projectId));
  const level = path === undefined ? roots : (findIn(roots, path)?.children ?? []);
  return prune(level, depth);
}

const DEMO_FILES: Record<string, string> = {
  "crates/pai-core/src/config.rs": `use std::fs;
use std::path::Path;

/// Lỗi có tên cho từng cách bộ nạp hỏng.
///
/// Gộp cả hai vào một \`String\` thì chỗ gọi không phân biệt được "không có tệp" với
/// "tệp sai cú pháp", và hai thứ đó cần hai câu trả lời khác nhau.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_norway::Error),
}

fn load(path: &Path) -> Result<Config, ConfigError> {
    let raw = fs::read_to_string(path).map_err(ConfigError::Io)?;
    serde_norway::from_str(&raw).map_err(ConfigError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_file() {
        let err = load(Path::new("/khong/ton/tai.yaml")).unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)));
    }
}
`,
  "ui/src/lib/registry.ts": `import type { Component } from "solid-js";

// Sổ đăng ký renderer. Không gian khoá là mở: tên lạ rơi vào fallback chứ không nổ.
const nodeRenderers = new Map<string, Component<unknown>>();

export function registerNode(kind: string, render: Component<unknown>): void {
  if (nodeRenderers.has(kind)) throw new Error(\`đã có renderer cho node "\${kind}"\`);
  nodeRenderers.set(kind, render);
}

export function nodeRenderer(kind: string): Component<unknown> | undefined {
  return nodeRenderers.get(kind);
}
`,
  "README.md": `# Harness

Bản viết lại Private AI thành một **coding & working agent** chạy trên máy người dùng:
lõi Rust, vỏ Tauri, giao diện SolidJS.

## Chạy

    cd harness/ui && npm install
    npm run tauri dev --prefix ui
`,
};

/**
 * Nội dung một tệp giả.
 *
 * `Cargo.lock` cố tình trả về `truncated: true`: cờ đó là một phần của hợp đồng, và một
 * cờ chỉ được bật ở phía Rust thì không ai nhìn thấy nó trông ra sao cho tới khi gặp một
 * tệp thật đủ lớn.
 */
export async function demoFile(path: string): Promise<FileView> {
  await new Promise<void>((resolve) => setTimeout(resolve, 140));
  // Gốc dài nhất trước: `/…/private-ai` là tiền tố của `/…/private-ai/harness`, nên cắt
  // theo thứ tự tuỳ tiện sẽ để lại `harness/…` và không tra ra tệp nào.
  const relative = [...demoProjects()]
    .sort((a, b) => b.path.length - a.path.length)
    .reduce((rest, entry) => (rest.startsWith(`${entry.path}/`) ? rest.slice(entry.path.length + 1) : rest), path);
  if (path.endsWith("Cargo.lock")) {
    const body = Array.from(
      { length: 60 },
      (_, index) => `[[package]]\nname = "crate-${index}"\nversion = "0.1.${index}"`,
    ).join("\n\n");
    return { text: body, lang: "toml", totalLines: 9421, truncated: true };
  }
  const known = DEMO_FILES[relative];
  const text =
    known ??
    `// ${path}\n//\n// Trang demo không mang theo nội dung thật của tệp này — nó chỉ dựng\n// đủ hình dạng để nhìn khung xem, số dòng và cuộn ngang.\n\nexport const duongDan = "${path}";\nexport const dong = ${path.length};\n`;
  return {
    text,
    lang: null,
    totalLines: text.split("\n").length,
    truncated: false,
  };
}

/** Mọi đường dẫn tệp của một dự án — thứ bảng ⌘P cần và cây nạp lười không có. */
export async function demoFilePaths(projectId: string): Promise<string[]> {
  await new Promise<void>((resolve) => setTimeout(resolve, 200));
  const out: string[] = [];
  const walk = (entries: TreeEntry[]) => {
    for (const entry of entries) {
      if (entry.isDir) walk(entry.children ?? []);
      else out.push(entry.path);
    }
  };
  walk(build(TREES[projectId] ?? [], demoRoot(projectId)));
  return out;
}
