import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { isDemo } from "./demo";
import {
  demoMcpCatalog,
  demoMcpServers,
  demoReloadMcp,
  demoRemoveMcpServer,
  demoSaveMcpServer,
  demoSetMcpEnabled,
} from "./fixtures/mcp";
import type { McpCatalogEntry, McpServer, McpServerInput } from "./protocol";

/**
 * Sáu lệnh MCP, chia hai nhóm theo cách xử lý lỗi — cùng ranh giới với `projects.ts`.
 *
 *   - `listMcpServers` và `mcpCatalog` chạy lúc mở màn hình, nuốt lỗi và trả rỗng.
 *   - `saveMcpServer`, `removeMcpServer`, `setMcpEnabled`, `reloadMcpServers` **ném ra
 *     ngoài**: cắm một server có thể mất vài giây, và một cú bấm không phản hồi sẽ được
 *     bấm lại — tức là cắm hai lần.
 *
 * Lưu ý một điều không thuộc về tầng này nhưng quyết định cách màn hình nói: kết quả tool
 * MCP là **nội dung không đáng tin** theo chính sách của lõi. Tầng bọc không làm gì với
 * điều đó; màn hình phải nói ra trước khi người dùng cắm một server lạ.
 */

export async function listMcpServers(): Promise<McpServer[]> {
  if (isDemo()) return demoMcpServers();
  if (!inTauri()) return [];
  try {
    return await invoke<McpServer[]>("list_mcp_servers");
  } catch (err) {
    console.error("không đọc được danh sách server MCP", err);
    return [];
  }
}

export async function mcpCatalog(): Promise<McpCatalogEntry[]> {
  if (isDemo()) return demoMcpCatalog();
  if (!inTauri()) return [];
  try {
    return await invoke<McpCatalogEntry[]>("mcp_catalog");
  } catch (err) {
    console.error("không đọc được danh mục MCP", err);
    return [];
  }
}

/**
 * Thêm hoặc sửa một server. Khoá định danh là `name`, nên đổi tên là thay thế.
 *
 * Trả về trạng thái *sau khi lõi thử cắm*, không phải bản ghi vừa lưu: người dùng cần
 * biết ngay server vừa thêm đã nối được chưa, và bắt họ tự bấm "Nạp lại" để biết là bắt
 * họ đoán.
 */
export function saveMcpServer(input: McpServerInput): Promise<McpServer> {
  if (isDemo()) return Promise.resolve(demoSaveMcpServer(input));
  return invoke<McpServer>("save_mcp_server", { input });
}

export function removeMcpServer(name: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoRemoveMcpServer(name));
  return invoke("remove_mcp_server", { name });
}

/** Tắt một server là **gỡ tool của nó khỏi mô hình**, không chỉ là ẩn một hàng đi. */
export function setMcpEnabled(name: string, enabled: boolean): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetMcpEnabled(name, enabled));
  return invoke("set_mcp_enabled", { name, enabled });
}

/** Cắm lại tất cả. Cách duy nhất để một server `failed` có cơ hội thứ hai. */
export function reloadMcpServers(): Promise<McpServer[]> {
  if (isDemo()) return Promise.resolve(demoReloadMcp());
  return invoke<McpServer[]>("reload_mcp_servers");
}

/** Một dòng trong bảng khai báo dán vào — `mcpServers` của tài liệu MCP ngoài kia. */
export interface ParsedMcp {
  name: string;
  input: McpServerInput;
  /** Số mục còn lại trong JSON. Dán một tệp có bốn server thì phải nói ra là đã bỏ ba. */
  rest: string[];
}

function emptyInput(): McpServerInput {
  return {
    name: "",
    transport: "stdio",
    command: "",
    args: [],
    env: {},
    cwd: null,
    url: "",
    headers: {},
    enabled: true,
  };
}

function stringMap(value: unknown): Record<string, string> {
  if (typeof value !== "object" || value === null) return {};
  const out: Record<string, string> = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    if (typeof raw === "string") out[key] = raw;
    else if (typeof raw === "number" || typeof raw === "boolean") out[key] = String(raw);
  }
  return out;
}

/**
 * Đọc một khai báo MCP dán từ tài liệu bên ngoài.
 *
 * Mọi README của một server MCP đều đưa ra đúng một hình dạng — `{"mcpServers": {…}}` —
 * và bắt người dùng gõ lại từng ô là bắt họ làm việc của máy. Hàm này cũng nhận một mục
 * trần (`{"command": …}`) và biến thể `{"servers": {…}}` mà một số tài liệu dùng, vì thứ
 * người ta dán thật sự là *đoạn họ bôi đen được*, không phải đoạn đúng chuẩn nhất.
 *
 * Ném `Error` với câu tiếng Việt thay vì trả `null`: chỗ gọi cần in ra **vì sao** JSON
 * không dùng được, và một `null` thì chỉ nói được là "hỏng".
 */
export function parseMcpJson(text: string): ParsedMcp {
  const trimmed = text.trim();
  if (trimmed === "") throw new Error("Chưa dán gì vào ô JSON.");

  let doc: unknown;
  try {
    doc = JSON.parse(trimmed);
  } catch (err) {
    throw new Error(`JSON không đọc được: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) {
    throw new Error("JSON phải là một đối tượng, ví dụ {\"mcpServers\": { … }}.");
  }

  const root = doc as Record<string, unknown>;
  const wrapper = root["mcpServers"] ?? root["servers"];
  const table =
    typeof wrapper === "object" && wrapper !== null
      ? (wrapper as Record<string, unknown>)
      : // Một mục trần cũng nhận, nhưng chỉ khi nó *trông như* một mục: có command hoặc
        // url. Nếu không thì người dán đã dán nhầm đoạn, và nói thẳng ra tốt hơn là im
        // lặng dựng một server rỗng.
        "command" in root || "url" in root
        ? { "": root }
        : {};

  const names = Object.keys(table);
  if (names.length === 0) {
    throw new Error("Không thấy mục nào trong \"mcpServers\". Dán cả khối, kể cả dấu ngoặc ngoài.");
  }

  const first = names[0]!;
  const body = table[first];
  if (typeof body !== "object" || body === null) {
    throw new Error(`Mục "${first}" không phải một đối tượng.`);
  }
  const entry = body as Record<string, unknown>;

  const input = emptyInput();
  input.name = first;
  input.command = typeof entry["command"] === "string" ? entry["command"] : "";
  input.args = Array.isArray(entry["args"])
    ? entry["args"].filter((arg): arg is string => typeof arg === "string")
    : [];
  input.env = stringMap(entry["env"]);
  input.cwd = typeof entry["cwd"] === "string" && entry["cwd"] !== "" ? entry["cwd"] : null;
  input.url = typeof entry["url"] === "string" ? entry["url"] : "";
  input.headers = stringMap(entry["headers"]);
  // Transport suy ra từ *cái có mặt*, không từ một trường `type` mà nửa số tài liệu quên
  // ghi. Có `url` mà không có `command` thì đó là http, mọi trường hợp còn lại là stdio.
  input.transport = input.command === "" && input.url !== "" ? "http" : "stdio";
  if (input.command === "" && input.url === "") {
    throw new Error(`Mục "${first}" không có "command" lẫn "url" — không biết chạy cái gì.`);
  }

  return { name: first, input, rest: names.slice(1) };
}
