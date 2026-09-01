"""Fetching the ASR model, without a terminal attached.

This used to live inside ``private-ai-asr setup``, printing a percentage to stdout. A
packaged app has no stdout anyone reads and no console script to invoke, so the download
moved here: one implementation, a callback instead of a ``print``, and the CLI supplies a
callback that prints. The alternative — a second downloader for the UI — is how the two
drift until only one of them validates the checksum.
"""

from __future__ import annotations

import errno
import hashlib
import http.client
import urllib.error
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any

__all__ = [
    "MODEL_SHA256_LENGTH",
    "MODEL_URL",
    "ModelDownloadError",
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


class ModelDownloadError(RuntimeError):
    """A download that failed for a reason worth telling the user about.

    Its message is already written for a person and is shown verbatim in the UI. The
    classification happens here because this is the only place that can tell a full disk
    from a dead network from a file the server garbled — by the time the exception
    reaches a widget, all three look alike and get reported as "check your network".
    """


def _reason(error: BaseException, target: Path) -> str:
    """One sentence naming what failed, and where that leaves the user."""
    if isinstance(error, urllib.error.HTTPError):
        return f"Máy chủ trả về lỗi {error.code}. Thử lại sau ít phút."
    if isinstance(error, urllib.error.URLError):
        return (
            f"Không kết nối được tới máy chủ mô hình ({error.reason}). Kiểm tra mạng rồi thử lại."
        )
    if isinstance(error, http.client.HTTPException | EOFError | ConnectionError):
        return "Kết nối bị ngắt giữa chừng. Thử tải lại."
    if isinstance(error, OSError):
        if error.errno == errno.ENOSPC:
            # No figure quoted: the row already shows the model's size beside this
            # sentence, and MIN_MODEL_BYTES is a sanity floor, not that size.
            return "Ổ đĩa đã đầy, không đủ chỗ cho tệp mô hình."
        if error.errno in (errno.EACCES, errno.EPERM, errno.EROFS):
            return f"Không có quyền ghi vào {target.parent}."
        return f"Không ghi được tệp mô hình: {error.strerror or error}."
    return f"Không tải được mô hình: {error}"


def checksum_path(target: Path) -> Path:
    return target.with_suffix(f"{target.suffix}.sha256")


def file_sha256(target: Path) -> str:
    digest = hashlib.sha256()
    with target.open("rb") as source:
        while chunk := source.read(CHUNK_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


class _RedirectRecorder(urllib.request.HTTPRedirectHandler):
    """Keeps the headers Hugging Face only sends on the redirect it then throws away.

    ``resolve/main/...`` answers with a 302 to a CDN, and the digest of the file lives on
    that 302 as ``X-Linked-ETag``. ``urlopen`` follows the hop and hands back only the
    CDN's headers, so a downloader that reads the response it ends up with never sees it.
    """

    def __init__(self) -> None:
        super().__init__()
        self.linked_etag = ""
        self.xet_hash = ""

    def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]  # noqa: ANN001, ANN201
        self.linked_etag = _hex_header(headers, "x-linked-etag") or self.linked_etag
        self.xet_hash = _hex_header(headers, "x-xet-hash") or self.xet_hash
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def _hex_header(headers: object, name: str) -> str:
    """The header's value if it looks like a SHA-256, otherwise nothing."""
    getter = getattr(headers, "get", None)
    value = (getter(name, "") if getter else "").strip('"').casefold()
    if len(value) != MODEL_SHA256_LENGTH:
        return ""
    return value if all(char in "0123456789abcdef" for char in value) else ""


def _expected_digest(response: object, recorder: _RedirectRecorder) -> str:
    """The SHA-256 the server declared for the file, or "" if it declared none.

    ``X-Linked-ETag`` first, because on a Xet-backed repository the CDN's own ``ETag`` is
    the **Xet** hash — 64 hex characters that are not the SHA-256 of the bytes. Trusting
    it meant every fresh download failed validation at exactly 100%. A plain ``ETag`` is
    still honoured for servers that publish the digest that way, unless it repeats the
    Xet hash the redirect already told us about.
    """
    if recorder.linked_etag:
        return recorder.linked_etag
    etag = _hex_header(getattr(response, "headers", None), "etag")
    return "" if etag and etag == recorder.xet_hash else etag


def _open(url: str) -> tuple[Any, str]:
    """Open ``url``, returning the response and the digest to validate the body against."""
    recorder = _RedirectRecorder()
    response = urllib.request.build_opener(recorder).open(url)  # noqa: S310
    return response, _expected_digest(response, recorder)


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
            raise ModelDownloadError(
                "Tệp mô hình đã có trên máy không khớp mã kiểm tra đã ghi. "
                f"Xoá {target} rồi tải lại."
            )
        manifest.write_text(f"{digest}\n", encoding="ascii")
        return

    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_suffix(f"{target.suffix}.part")
    digest = hashlib.sha256()
    copied = 0
    try:
        response, expected = _open(url)
        with response, partial.open("wb") as destination:
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
    except InterruptedError:
        # A subclass of OSError, so it has to be caught ahead of the clause below or the
        # user's own cancellation comes back as "không ghi được tệp mô hình".
        partial.unlink(missing_ok=True)
        raise
    except (OSError, http.client.HTTPException, EOFError) as error:
        # URLError and HTTPError are OSError subclasses, so this one clause covers the
        # network and the disk both; ``_reason`` is what tells them apart.
        partial.unlink(missing_ok=True)
        raise ModelDownloadError(_reason(error, target)) from error
    except BaseException:
        # KeyboardInterrupt and task cancellation are not failures to explain, only to
        # clean up after.
        partial.unlink(missing_ok=True)
        raise

    actual = digest.hexdigest()
    if expected and actual != expected:
        partial.unlink(missing_ok=True)
        raise ModelDownloadError(
            "Tệp tải về không khớp mã kiểm tra của máy chủ. Mạng hoặc proxy có thể đã "
            "sửa nội dung trên đường truyền; thử lại, đổi mạng nếu vẫn hỏng."
        )
    try:
        partial.replace(target)
        checksum_path(target).write_text(f"{actual}\n", encoding="ascii")
    except OSError as error:
        partial.unlink(missing_ok=True)
        raise ModelDownloadError(_reason(error, target)) from error
