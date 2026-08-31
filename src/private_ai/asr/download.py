"""Fetching the ASR model, without a terminal attached.

This used to live inside ``private-ai-asr setup``, printing a percentage to stdout. A
packaged app has no stdout anyone reads and no console script to invoke, so the download
moved here: one implementation, a callback instead of a ``print``, and the CLI supplies a
callback that prints. The alternative — a second downloader for the UI — is how the two
drift until only one of them validates the checksum.
"""

from __future__ import annotations

import hashlib
import urllib.request
from collections.abc import Callable
from pathlib import Path

__all__ = [
    "MODEL_SHA256_LENGTH",
    "MODEL_URL",
    "ProgressCallback",
    "checksum_path",
    "download",
    "file_sha256",
]

MODEL_URL = (
    "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/"
    "resolve/main/nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf"
)

# A file smaller than this is a truncated download or an HTML error page, never the model.
MIN_MODEL_BYTES = 100_000_000

MODEL_SHA256_LENGTH = 64

CHUNK_BYTES = 1024 * 1024

# ``(copied_bytes, total_bytes)``; ``total`` is 0 when the server sent no length.
ProgressCallback = Callable[[int, int], None]


def checksum_path(target: Path) -> Path:
    return target.with_suffix(f"{target.suffix}.sha256")


def file_sha256(target: Path) -> str:
    digest = hashlib.sha256()
    with target.open("rb") as source:
        while chunk := source.read(CHUNK_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


def _expected_digest(response: object) -> str:
    """Hugging Face publishes the file's SHA-256 as its ETag.

    So the download validates itself without a second request for a manifest. Anything
    that is not 64 hex characters is some other kind of ETag and is ignored.
    """
    headers = getattr(response, "headers", None)
    etag = (headers.get("etag", "") if headers else "").strip('"').casefold()
    if len(etag) != MODEL_SHA256_LENGTH:
        return ""
    return etag if all(char in "0123456789abcdef" for char in etag) else ""


def download(
    url: str,
    target: Path,
    *,
    force: bool = False,
    on_progress: ProgressCallback | None = None,
    should_cancel: Callable[[], bool] | None = None,
) -> None:
    """Fetch the model, verifying it against the digest the server declared.

    Downloads into a ``.part`` file and renames on success, so an interrupted run leaves
    nothing that looks like a finished model — a half-written GGUF loads far enough to
    fail somewhere much less obvious.
    """
    if not force and target.is_file() and target.stat().st_size > MIN_MODEL_BYTES:
        digest = file_sha256(target)
        manifest = checksum_path(target)
        if manifest.is_file() and manifest.read_text(encoding="ascii").strip() != digest:
            raise RuntimeError("Existing ASR model failed SHA-256 integrity validation")
        manifest.write_text(f"{digest}\n", encoding="ascii")
        return

    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_suffix(f"{target.suffix}.part")
    digest = hashlib.sha256()
    copied = 0
    try:
        with urllib.request.urlopen(url) as response, partial.open("wb") as destination:  # noqa: S310
            expected = _expected_digest(response)
            total = int(response.headers.get("content-length", "0"))
            if on_progress:
                on_progress(0, total)
            while chunk := response.read(CHUNK_BYTES):
                if should_cancel and should_cancel():
                    raise InterruptedError("Đã huỷ tải mô hình")
                destination.write(chunk)
                digest.update(chunk)
                copied += len(chunk)
                if on_progress:
                    on_progress(copied, total)
    except BaseException:
        partial.unlink(missing_ok=True)
        raise

    actual = digest.hexdigest()
    if expected and actual != expected:
        partial.unlink(missing_ok=True)
        raise RuntimeError("Downloaded ASR model failed SHA-256 integrity validation")
    partial.replace(target)
    checksum_path(target).write_text(f"{actual}\n", encoding="ascii")
