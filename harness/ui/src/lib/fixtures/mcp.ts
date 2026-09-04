import type { McpCatalogEntry, McpServer, McpServerInput } from "../protocol";

/** Fake MCP servers for `?demo=1`: four rows, one per `McpState`. Tool names carry the `ext.<server>.` prefix,
 * because that is the name the model sees and the name that appears in the transcript. */

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
      // A real "Node not installed" failure as the core would word it: command, errno, and the action to take.
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

/** Shortened to one line; the core does this, so the fixture must match. */
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
    // A newly added server always starts at `connecting`; jumping straight to `connected` would teach a lie.
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
    // Disabling removes the tools from the model, so the tool list must empty out too.
    hit.state = "disabled";
    hit.tools = [];
    hit.error = null;
  }
}

/** Reload: `connecting` servers settle to `connected`, and `failed` ones get another attempt. */
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
        // A required non-secret variable beside a secret one; the two fields must look different.
        { key: "SENTRY_ORG", label: "Tên tổ chức", required: true, secret: false },
        { key: "SENTRY_PROJECT", label: "Tên dự án (tuỳ chọn)", required: false, secret: false },
      ],
      homepage: "https://github.com/modelcontextprotocol/servers",
      requires: ["python"],
      url: null,
    },
  ];
}
