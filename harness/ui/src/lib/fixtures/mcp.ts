import type { McpCatalogEntry, McpServer, McpServerInput } from "../protocol";

/**
 * Server MCP giả cho `?demo=1`.
 *
 * Bốn hàng, đúng bốn trạng thái của `McpState`: `connected` với một rổ tool thật,
 * `failed` kèm thông điệp lỗi nguyên văn, `disabled`, và `connecting`. Ba trong bốn cái
 * đó chỉ tồn tại trong vài giây hoặc chỉ khi máy người dùng thiếu một thứ gì đó — nghĩa
 * là không dựng ra ở đây thì không có cách nào nhìn thấy chúng trước khi phát hành.
 *
 * Tên tool mang sẵn tiền tố `ext.<server>.`: đó là tên **mô hình thật sự thấy**, và cũng
 * là tên xuất hiện trong bản ghi khi một lượt gọi tới nó. Hiện tên từ xa thay vào đó sẽ
 * dựng một danh sách không tra cứu ngược được từ bản ghi.
 */

let store: McpServer[] | null = null;

function seed(): McpServer[] {
  return [
    {
      name: "github",
      transport: "stdio",
      target: "npx -y @modelcontextprotocol/server-github",
      enabled: true,
      state: "connected",
      tools: [
        "ext.github.search_repositories",
        "ext.github.get_file_contents",
        "ext.github.create_issue",
        "ext.github.list_pull_requests",
        "ext.github.create_pull_request_review",
        "ext.github.merge_pull_request",
      ],
      error: null,
    },
    {
      name: "filesystem",
      transport: "stdio",
      target: "npx -y @modelcontextprotocol/server-filesystem /Users/vinhpx/Documents",
      enabled: true,
      state: "failed",
      tools: [],
      // Lỗi thật của một máy chưa cài Node, viết đúng như lõi sẽ đưa ra: có tên lệnh, có
      // mã lỗi hệ thống, và có việc phải làm. Một câu chung chung ở đây là một server
      // hỏng vĩnh viễn.
      error: "spawn npx ENOENT — không tìm thấy `npx` trong PATH. Cài Node.js (hoặc sửa đường dẫn lệnh) rồi bấm Nạp lại.",
    },
    {
      name: "jira",
      transport: "http",
      target: "https://mcp.atlassian.com/v1/sse",
      enabled: false,
      state: "disabled",
      tools: [],
      error: null,
    },
    {
      name: "postgres",
      transport: "stdio",
      target: "docker run -i --rm mcp/postgres postgresql://localhost/pai",
      enabled: true,
      state: "connecting",
      tools: [],
      error: null,
    },
  ];
}

function all(): McpServer[] {
  store ??= seed();
  return store;
}

export function demoMcpServers(): McpServer[] {
  return all().map((entry) => ({ ...entry, tools: [...entry.tools] }));
}

/** Rút gọn để hiện trên một dòng — lõi làm việc này, bản mẫu phải làm giống. */
function targetOf(input: McpServerInput): string {
  return input.transport === "http"
    ? input.url
    : [input.command, ...input.args].filter((part) => part !== "").join(" ");
}

export function demoSaveMcpServer(input: McpServerInput): McpServer {
  const list = all();
  const saved: McpServer = {
    name: input.name,
    transport: input.transport,
    target: targetOf(input),
    enabled: input.enabled,
    // Server mới cắm luôn bắt đầu ở `connecting`: lõi thật cũng không biết kết quả trước
    // khi tiến trình con trả lời, và một hàng nhảy thẳng sang `connected` sẽ dạy người
    // dùng rằng trạng thái đó là tức thời.
    state: input.enabled ? "connecting" : "disabled",
    tools: [],
    error: null,
  };
  const at = list.findIndex((entry) => entry.name === input.name);
  if (at < 0) list.push(saved);
  else list[at] = saved;
  return { ...saved };
}

export function demoRemoveMcpServer(name: string): void {
  store = all().filter((entry) => entry.name !== name);
}

export function demoSetMcpEnabled(name: string, enabled: boolean): void {
  const hit = all().find((entry) => entry.name === name);
  if (!hit) return;
  hit.enabled = enabled;
  if (enabled) {
    hit.state = "connecting";
  } else {
    // Tắt là gỡ tool khỏi mô hình, nên danh sách tool phải rỗng đi theo. Giữ lại danh
    // sách cũ sẽ vẽ ra một server đang tắt mà vẫn "có 6 tool" — không đúng với cái mô
    // hình nhìn thấy.
    hit.state = "disabled";
    hit.tools = [];
    hit.error = null;
  }
}

/** Nạp lại: server `connecting` chốt lại thành `connected`, `failed` được thử lần nữa. */
export function demoReloadMcp(): McpServer[] {
  for (const entry of all()) {
    if (!entry.enabled) continue;
    if (entry.name === "postgres") {
      entry.state = "connected";
      entry.tools = ["ext.postgres.query", "ext.postgres.list_schemas", "ext.postgres.describe_table"];
      entry.error = null;
    } else if (entry.state === "connecting") {
      entry.state = "connected";
      entry.error = null;
    }
  }
  return demoMcpServers();
}

export function demoMcpCatalog(): McpCatalogEntry[] {
  return [
    {
      id: "github",
      name: "GitHub",
      summary: "Đọc issue, pull request và tệp trong kho GitHub.",
      command: "",
      args: [],
      env: [
        {
          key: "Authorization",
          label: "Personal access token của GitHub",
          required: true,
          secret: true,
        },
      ],
      homepage: "https://github.com/github/github-mcp-server",
      requires: [],
      url: "https://api.githubcopilot.com/mcp/",
    },
    {
      id: "filesystem",
      name: "Hệ tệp",
      summary: "Đọc và ghi tệp ngoài thư mục dự án.",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem", "."],
      env: [],
      homepage: "https://github.com/modelcontextprotocol/servers",
      requires: ["node"],
      url: null,
    },
    {
      id: "postgres",
      name: "PostgreSQL",
      summary: "Truy vấn chỉ-đọc và xem lược đồ Postgres.",
      command: "docker",
      args: ["run", "-i", "--rm", "mcp/postgres"],
      env: [
        { key: "DATABASE_URL", label: "Chuỗi kết nối", required: true, secret: true },
      ],
      homepage: "https://github.com/modelcontextprotocol/servers",
      requires: ["docker"],
      url: null,
    },
    {
      id: "sentry",
      name: "Sentry",
      summary: "Kéo về chi tiết sự cố và ngăn xếp lỗi.",
      command: "uvx",
      args: ["mcp-server-sentry"],
      env: [
        { key: "SENTRY_AUTH_TOKEN", label: "Auth token", required: true, secret: true },
        // Một biến bắt buộc *không* bí mật, cạnh một biến bí mật: hai ô phải trông khác
        // nhau, và chỉ có bộ mẫu này mới bày được cả hai cạnh nhau.
        { key: "SENTRY_ORG", label: "Tên tổ chức", required: true, secret: false },
        { key: "SENTRY_PROJECT", label: "Tên dự án (tuỳ chọn)", required: false, secret: false },
      ],
      homepage: "https://github.com/modelcontextprotocol/servers",
      requires: ["python"],
      url: null,
    },
  ];
}
