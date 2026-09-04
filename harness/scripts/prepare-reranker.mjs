#!/usr/bin/env node

/**
 * Materialize the pinned INT8 ONNX export of BAAI/bge-reranker-v2-m3.
 *
 * The 571 MB graph is streamed to disk instead of buffered in Node. Every file is
 * content-addressed here so a moved Hugging Face branch cannot alter a release.
 */

import { createHash } from "node:crypto";
import { createReadStream, createWriteStream } from "node:fs";
import { mkdir, rename, stat, unlink } from "node:fs/promises";
import { dirname, join } from "node:path";
import { Readable, Transform, Writable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const REPOSITORY = "onnx-community/bge-reranker-v2-m3-ONNX";
const REVISION = "a3046abee880d6e78833e4e885939754355156bd";
const MAX_FILE_BYTES = 600 * 1024 * 1024;
const outputDir = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "app",
  "models",
  "bge-reranker-v2-m3",
);

const files = [
  {
    remote: "onnx/model_quantized.onnx",
    local: "model_quantized.onnx",
    bytes: 570_727_094,
    sha256: "912fc1215c2dbff6499700534bd8d31253af01573861abbfc43afd1fab6cce5d",
  },
  {
    remote: "tokenizer.json",
    local: "tokenizer.json",
    bytes: 17_082_900,
    sha256: "8bf8afbfd11306bd872018c53bfdf2e160a56f8edbcf49933324404791c148d3",
  },
  {
    remote: "config.json",
    local: "config.json",
    bytes: 848,
    sha256: "122e922dcfed6503c8721e6fe1daf090340c3d95ca7f3aa3a72730b321a51cfd",
  },
  {
    remote: "special_tokens_map.json",
    local: "special_tokens_map.json",
    bytes: 964,
    sha256: "8c785abebea9ae3257b61681b4e6fd8365ceafde980c21970d001e834cf10835",
  },
  {
    remote: "tokenizer_config.json",
    local: "tokenizer_config.json",
    bytes: 1_203,
    sha256: "b87c8703482b0300d3da30e201519aa641f6a450f5eb5bf1e624afbf70c74d80",
  },
];

async function digest(path) {
  const hash = createHash("sha256");
  await pipeline(createReadStream(path), new Writable({
    write(chunk, _encoding, done) {
      hash.update(chunk);
      done();
    },
  }));
  return hash.digest("hex");
}

async function valid(path, expected) {
  try {
    const info = await stat(path);
    return info.isFile() && info.size === expected.bytes && (await digest(path)) === expected.sha256;
  } catch {
    return false;
  }
}

async function prepare(spec) {
  const destination = join(outputDir, spec.local);
  if (await valid(destination, spec)) {
    process.stdout.write(`Có sẵn reranker/${spec.local}\n`);
    return;
  }

  const url = `https://huggingface.co/${REPOSITORY}/resolve/${REVISION}/${spec.remote}?download=true`;
  process.stdout.write(`Tải ${spec.remote} (${Math.ceil(spec.bytes / 1024 / 1024)} MiB)\n`);
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || response.body === null) {
    throw new Error(`tải thất bại: HTTP ${response.status} ${url}`);
  }
  const declared = Number(response.headers.get("content-length") || 0);
  if (declared > MAX_FILE_BYTES) throw new Error(`${spec.remote} vượt trần tải xuống`);

  const temporary = `${destination}.tmp-${process.pid}`;
  const hash = createHash("sha256");
  let bytes = 0;
  const meter = new Transform({
    transform(chunk, _encoding, done) {
      bytes += chunk.length;
      hash.update(chunk);
      if (bytes > MAX_FILE_BYTES) done(new Error(`${spec.remote} vượt trần tải xuống`));
      else done(null, chunk);
    },
  });
  try {
    await pipeline(Readable.fromWeb(response.body), meter, createWriteStream(temporary, { mode: 0o644 }));
    const actual = hash.digest("hex");
    if (bytes !== spec.bytes || actual !== spec.sha256) {
      throw new Error(
        `${spec.remote} không khớp bản đã ghim: ${bytes} byte, SHA-256 ${actual}`,
      );
    }
    try {
      await unlink(destination);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    await rename(temporary, destination);
  } catch (error) {
    try {
      await unlink(temporary);
    } catch (cleanupError) {
      if (cleanupError.code !== "ENOENT") process.stderr.write(`${cleanupError.message}\n`);
    }
    throw error;
  }
}

async function main() {
  await mkdir(outputDir, { recursive: true });
  for (const spec of files) await prepare(spec);
}

main().catch((error) => {
  process.stderr.write(`Không chuẩn bị được ONNX reranker: ${error.message}\n`);
  process.exitCode = 1;
});
