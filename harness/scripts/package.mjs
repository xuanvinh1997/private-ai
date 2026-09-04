#!/usr/bin/env node

/** One production entry point: prepare verified sidecars, then ask Tauri to bundle them. */
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const executable = join(
  root,
  "ui",
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);
const result = spawnSync(
  executable,
  ["build", "--config", "app/tauri.production.conf.json", ...process.argv.slice(2)],
  { cwd: root, env: process.env, stdio: "inherit", shell: process.platform === "win32" },
);
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
