import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./agent";
import { isDemo } from "./demo";
import { S, t } from "./i18n";
import {
  demoMcpCatalog,
  demoMcpServers,
  demoReloadMcp,
  demoRemoveMcpServer,
  demoSaveMcpServer,
  demoSetMcpEnabled,
} from "./fixtures/mcp";
import type { McpCatalogEntry, McpServer, McpServerInput } from "./protocol";

/** The MCP commands, split like `projects.ts`: screen-load calls swallow errors, click-driven ones throw, since
 * connecting can take seconds and an unanswered click gets clicked again. MCP tool output is untrusted content. */

export async function listMcpServers(): Promise<McpServer[]> {
  if (isDemo()) return demoMcpServers();
  if (!inTauri()) return [];
  try {
    return await invoke<McpServer[]>("list_mcp_servers");
  } catch (err) {
    console.error("failed to list MCP servers", err);
    return [];
  }
}

export async function mcpCatalog(): Promise<McpCatalogEntry[]> {
  if (isDemo()) return demoMcpCatalog();
  if (!inTauri()) return [];
  try {
    return await invoke<McpCatalogEntry[]>("mcp_catalog");
  } catch (err) {
    console.error("failed to read MCP catalog", err);
    return [];
  }
}

/** Add or edit a server, keyed by `name`, so renaming replaces; returns the state after the core tried to connect. */
export function saveMcpServer(input: McpServerInput): Promise<McpServer> {
  if (isDemo()) return Promise.resolve(demoSaveMcpServer(input));
  return invoke<McpServer>("save_mcp_server", { input });
}

export function removeMcpServer(name: string): Promise<void> {
  if (isDemo()) return Promise.resolve(demoRemoveMcpServer(name));
  return invoke("remove_mcp_server", { name });
}

/** Disabling a server removes its tools from the model; it does not merely hide a row. */
export function setMcpEnabled(name: string, enabled: boolean): Promise<void> {
  if (isDemo()) return Promise.resolve(demoSetMcpEnabled(name, enabled));
  return invoke("set_mcp_enabled", { name, enabled });
}

/** Reconnect everything: the only way a `failed` server gets a second chance. */
export function reloadMcpServers(): Promise<McpServer[]> {
  if (isDemo()) return Promise.resolve(demoReloadMcp());
  return invoke<McpServer[]>("reload_mcp_servers");
}

/** One entry of a pasted declaration, i.e. the `mcpServers` table used by MCP documentation. */
export interface ParsedMcp {
  name: string;
  input: McpServerInput;
  /** The remaining entries in the JSON; pasting four servers must say that three were skipped. */
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

/** Parse an MCP declaration pasted from documentation: accepts `mcpServers`, `servers`, or a bare entry, and
 * throws an `Error` carrying a translated reason, because the caller must print why the JSON is unusable. */
export function parseMcpJson(text: string): ParsedMcp {
  const trimmed = text.trim();
  if (trimmed === "") throw new Error(t(S.mcp.json.empty));

  let doc: unknown;
  try {
    doc = JSON.parse(trimmed);
  } catch (err) {
    throw new Error(
      t(S.mcp.json.unreadable, { msg: err instanceof Error ? err.message : String(err) }),
    );
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) {
    throw new Error(t(S.mcp.json.notObject));
  }

  const root = doc as Record<string, unknown>;
  const wrapper = root["mcpServers"] ?? root["servers"];
  const table =
    typeof wrapper === "object" && wrapper !== null
      ? (wrapper as Record<string, unknown>)
      : // A bare entry is accepted only when it looks like one (a command or a url); otherwise say so.
        "command" in root || "url" in root
        ? { "": root }
        : {};

  const names = Object.keys(table);
  if (names.length === 0) {
    throw new Error(t(S.mcp.json.noEntries));
  }

  const first = names[0]!;
  const body = table[first];
  if (typeof body !== "object" || body === null) {
    throw new Error(t(S.mcp.json.entryNotObject, { name: first }));
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
  // Transport inferred from what is present, not a `type` field half the docs omit: url without command is http.
  input.transport = input.command === "" && input.url !== "" ? "http" : "stdio";
  if (input.command === "" && input.url === "") {
    throw new Error(t(S.mcp.json.entryNoTarget, { name: first }));
  }

  return { name: first, input, rest: names.slice(1) };
}
