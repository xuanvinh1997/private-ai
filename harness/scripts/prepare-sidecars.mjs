#!/usr/bin/env node

/**
 * Download the exact Qdrant and SurrealDB executables that Tauri will embed.
 *
 * Tauri requires `<name>-<target-triple>[.exe]` beside tauri.conf.json. Archives are
 * pinned by SHA-256, extracted without third-party packages, and cached only while the
 * extracted executable still matches the locally recorded digest.
 */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, mkdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, inflateRawSync } from "node:zlib";

const QDRANT_VERSION = "v1.19.0";
const SURREAL_VERSION = "v3.2.4";
const MAX_DOWNLOAD_BYTES = 300 * 1024 * 1024;

const scriptDir = dirname(fileURLToPath(import.meta.url));
const outputDir = join(scriptDir, "..", "app", "binaries");

const qdrant = (asset, sha256, archive) => ({
  url: `https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/${asset}`,
  sha256,
  archive,
  member: asset.includes("windows") ? "qdrant.exe" : "qdrant",
  version: QDRANT_VERSION,
});

const surreal = (asset, sha256, archive) => ({
  url: `https://download.surrealdb.com/${SURREAL_VERSION}/${asset}`,
  sha256,
  archive,
  member: asset.includes("windows") ? "surreal.exe" : "surreal",
  version: SURREAL_VERSION,
});

const targets = {
  "aarch64-apple-darwin": {
    qdrant: qdrant(
      "qdrant-aarch64-apple-darwin.tar.gz",
      "4e279a80cc1ebe73e859318ff86375af54c123887dd7ae46605c0eb6cb7c44e8",
      "tgz",
    ),
    surreal: surreal(
      `surreal-${SURREAL_VERSION}.darwin-arm64.tgz`,
      "8d703e9c5ed12e509ec7eb9b17385d3cac440077f93980d5c98b57c2d99cbbe8",
      "tgz",
    ),
  },
  "x86_64-apple-darwin": {
    qdrant: qdrant(
      "qdrant-x86_64-apple-darwin.tar.gz",
      "e7afefcc125856157b33c6184c00ddee3f1d5b112474649070592d9fdd9a3f54",
      "tgz",
    ),
    surreal: surreal(
      `surreal-${SURREAL_VERSION}.darwin-amd64.tgz`,
      "bcbb5cabf1695cda6a5d0d5866e54f020bd64f7d641abb932d946e2b8dbb0ad7",
      "tgz",
    ),
  },
  "x86_64-pc-windows-msvc": {
    qdrant: qdrant(
      "qdrant-x86_64-pc-windows-msvc.zip",
      "980cb2e1ae771155cf211da8c0a8a9206b6482bd4effdc4db994d3adb707b087",
      "zip",
    ),
    surreal: surreal(
      `surreal-${SURREAL_VERSION}.windows-amd64.exe`,
      "ad200ea01c3cb99f84617c60d61caf40c2e13d72cc4ed08378387d2d74f8fbf4",
      "raw",
    ),
  },
  "aarch64-unknown-linux-gnu": {
    qdrant: qdrant(
      "qdrant-aarch64-unknown-linux-musl.tar.gz",
      "8986afbbff9ac32d6e2dbe5cabec80565f613f777126096a461ba066573d3245",
      "tgz",
    ),
    surreal: surreal(
      `surreal-${SURREAL_VERSION}.linux-arm64.tgz`,
      "64d9f9c6138df768bf04c0d3637d2ca3655022819ae0b1772a0d62f2fb3f5f03",
      "tgz",
    ),
  },
  "x86_64-unknown-linux-gnu": {
    qdrant: qdrant(
      "qdrant-x86_64-unknown-linux-musl.tar.gz",
      "9ec667456443463eee390e43cd36988af6b730c6db807b4e39f57c303d0264a3",
      "tgz",
    ),
    surreal: surreal(
      `surreal-${SURREAL_VERSION}.linux-amd64.tgz`,
      "aaf9c8d388248db63e10300385c94ec9f85ef4430e79f9569886045d896df369",
      "tgz",
    ),
  },
};

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function targetArgument() {
  const at = process.argv.indexOf("--target");
  if (at >= 0) {
    const value = process.argv[at + 1];
    if (!value || value.startsWith("--")) throw new Error("--target cần một Rust target triple");
    return value;
  }
  return (
    process.env.TAURI_ENV_TARGET_TRIPLE ||
    execFileSync("rustc", ["--print", "host-tuple"], { encoding: "utf8" }).trim()
  );
}

function tarMember(archive, wanted) {
  const tar = gunzipSync(archive);
  for (let offset = 0; offset + 512 <= tar.length; ) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const string = (start, length) =>
      header.subarray(start, start + length).toString("utf8").replace(/\0.*$/s, "");
    const name = [string(345, 155), string(0, 100)].filter(Boolean).join("/");
    const rawSize = string(124, 12).trim();
    const size = Number.parseInt(rawSize || "0", 8);
    if (!Number.isSafeInteger(size) || size < 0) throw new Error("tar có kích thước member không hợp lệ");
    const data = offset + 512;
    if (basename(name) === wanted) return tar.subarray(data, data + size);
    offset = data + Math.ceil(size / 512) * 512;
  }
  throw new Error(`archive không chứa ${wanted}`);
}

function zipMember(archive, wanted) {
  let end = -1;
  for (let at = archive.length - 22; at >= Math.max(0, archive.length - 65_557); at -= 1) {
    if (archive.readUInt32LE(at) === 0x06054b50) {
      end = at;
      break;
    }
  }
  if (end < 0) throw new Error("ZIP thiếu end-of-central-directory");
  const entries = archive.readUInt16LE(end + 10);
  let cursor = archive.readUInt32LE(end + 16);
  for (let index = 0; index < entries; index += 1) {
    if (archive.readUInt32LE(cursor) !== 0x02014b50) throw new Error("ZIP central directory bị hỏng");
    const method = archive.readUInt16LE(cursor + 10);
    const compressedSize = archive.readUInt32LE(cursor + 20);
    const expandedSize = archive.readUInt32LE(cursor + 24);
    const nameLength = archive.readUInt16LE(cursor + 28);
    const extraLength = archive.readUInt16LE(cursor + 30);
    const commentLength = archive.readUInt16LE(cursor + 32);
    const localOffset = archive.readUInt32LE(cursor + 42);
    const name = archive.subarray(cursor + 46, cursor + 46 + nameLength).toString("utf8");
    if (basename(name.replaceAll("\\", "/")) === wanted) {
      if (archive.readUInt32LE(localOffset) !== 0x04034b50) throw new Error("ZIP local header bị hỏng");
      const localNameLength = archive.readUInt16LE(localOffset + 26);
      const localExtraLength = archive.readUInt16LE(localOffset + 28);
      const start = localOffset + 30 + localNameLength + localExtraLength;
      const compressed = archive.subarray(start, start + compressedSize);
      const output = method === 0 ? compressed : method === 8 ? inflateRawSync(compressed) : null;
      if (output === null) throw new Error(`ZIP dùng compression method chưa hỗ trợ: ${method}`);
      if (output.length !== expandedSize) throw new Error("ZIP member có kích thước sai");
      return output;
    }
    cursor += 46 + nameLength + extraLength + commentLength;
  }
  throw new Error(`archive không chứa ${wanted}`);
}

async function download(spec) {
  process.stdout.write(`Tải ${spec.url}\n`);
  const response = await fetch(spec.url, { redirect: "follow" });
  if (!response.ok) throw new Error(`tải thất bại: HTTP ${response.status} ${spec.url}`);
  const declared = Number(response.headers.get("content-length") || 0);
  if (declared > MAX_DOWNLOAD_BYTES) throw new Error(`gói vượt trần ${MAX_DOWNLOAD_BYTES} byte`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length > MAX_DOWNLOAD_BYTES) throw new Error(`gói vượt trần ${MAX_DOWNLOAD_BYTES} byte`);
  const actual = sha256(bytes);
  if (actual !== spec.sha256) {
    throw new Error(`SHA-256 sai cho ${spec.url}: nhận ${actual}, cần ${spec.sha256}`);
  }
  if (spec.archive === "raw") return bytes;
  if (spec.archive === "tgz") return tarMember(bytes, spec.member);
  if (spec.archive === "zip") return zipMember(bytes, spec.member);
  throw new Error(`kiểu archive chưa hỗ trợ: ${spec.archive}`);
}

async function cached(destination, spec) {
  const stampPath = `${destination}.json`;
  try {
    const [binary, stamp] = await Promise.all([
      readFile(destination),
      readFile(stampPath, "utf8").then(JSON.parse),
    ]);
    return stamp.sourceSha256 === spec.sha256 && stamp.binarySha256 === sha256(binary);
  } catch {
    return false;
  }
}

async function atomicExecutable(destination, bytes, stamp) {
  const temporary = `${destination}.tmp-${process.pid}`;
  await writeFile(temporary, bytes, { mode: 0o755 });
  if (process.platform !== "win32") await chmod(temporary, 0o755);
  try {
    await unlink(destination);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  await rename(temporary, destination);
  await writeFile(`${destination}.json`, `${JSON.stringify(stamp, null, 2)}\n`);
}

function destination(name, target) {
  return join(outputDir, `${name}-${target}${target.includes("windows") ? ".exe" : ""}`);
}

async function prepareOne(name, target, spec) {
  const output = destination(name, target);
  if (await cached(output, spec)) {
    process.stdout.write(`Có sẵn ${basename(output)} (${spec.version})\n`);
    return output;
  }
  const binary = await download(spec);
  await atomicExecutable(output, binary, {
    service: name,
    version: spec.version,
    source: spec.url,
    sourceSha256: spec.sha256,
    binarySha256: sha256(binary),
  });
  process.stdout.write(`Đã chuẩn bị ${basename(output)}\n`);
  return output;
}

async function prepareUniversal() {
  if (process.platform !== "darwin") throw new Error("universal-apple-darwin chỉ dựng được trên macOS");
  for (const name of ["qdrant", "surreal"]) {
    const arm = await prepareOne(name, "aarch64-apple-darwin", targets["aarch64-apple-darwin"][name]);
    const intel = await prepareOne(name, "x86_64-apple-darwin", targets["x86_64-apple-darwin"][name]);
    const output = destination(name, "universal-apple-darwin");
    const temporary = `${output}.tmp-${process.pid}`;
    execFileSync("lipo", ["-create", arm, intel, "-output", temporary], { stdio: "inherit" });
    execFileSync("lipo", [temporary, "-verify_arch", "arm64", "x86_64"], { stdio: "inherit" });
    const binary = await readFile(temporary);
    await atomicExecutable(output, binary, {
      service: name,
      version: `${targets["aarch64-apple-darwin"][name].version} universal`,
      source: "lipo(aarch64-apple-darwin,x86_64-apple-darwin)",
      sourceSha256: [
        targets["aarch64-apple-darwin"][name].sha256,
        targets["x86_64-apple-darwin"][name].sha256,
      ].join("+"),
      binarySha256: sha256(binary),
    });
    process.stdout.write(`Đã chuẩn bị ${basename(output)}\n`);
  }
}

async function main() {
  await mkdir(outputDir, { recursive: true });
  const target = targetArgument();
  if (target === "universal-apple-darwin") {
    await prepareUniversal();
    return;
  }
  const specs = targets[target];
  if (!specs) {
    throw new Error(
      `chưa có sidecar cho ${target}; hỗ trợ: ${Object.keys(targets).join(", ")}, universal-apple-darwin`,
    );
  }
  await Promise.all([
    prepareOne("qdrant", target, specs.qdrant),
    prepareOne("surreal", target, specs.surreal),
  ]);
}

main().catch((error) => {
  process.stderr.write(`Không chuẩn bị được sidecar: ${error.message}\n`);
  process.exitCode = 1;
});
