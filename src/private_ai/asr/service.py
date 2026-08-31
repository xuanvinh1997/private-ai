"""Speech recognition backed by a local transcribe.cpp build.

Two paths share one model directory. The batch path shells out to ``transcribe-cli``
after ffmpeg normalises whatever the microphone produced into 16 kHz mono PCM; the
streaming path loads the shared library through the ``transcribe_cpp`` binding and keeps
the model resident so partial text can appear while the user is still speaking.
"""

from __future__ import annotations

import array
import asyncio
import importlib
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.gpu_lease import GpuLeaseManager

ASR_MODEL_NAME = "nemotron-3.5-asr-streaming-0.6b"
# How long ``close`` waits for a session to give the lock back before taking the model
# down regardless. Long enough for a finalize in flight, short enough to quit on.
CLOSE_TIMEOUT_SECONDS = 5.0

logger = logging.getLogger(__name__)


class AsrUnavailable(RuntimeError):
    pass


class AsrStream:
    """Async owner for one native transcribe.cpp streaming session."""

    def __init__(
        self,
        service: AsrService,
        session: Any,
        stream: Any,
        language: str,
    ) -> None:
        self._service = service
        self._session = session
        self._stream = stream
        self.language = language
        self._closed = False

    async def feed(self, content: bytes) -> dict[str, Any]:
        if self._closed:
            raise AsrUnavailable("Phiên nhận dạng giọng nói đã đóng")
        if not content or len(content) % 4:
            raise ValueError("Nhận dạng giọng nói theo luồng cần khung PCM float32 little-endian")
        try:
            return await asyncio.to_thread(self._feed_sync, content)
        except BaseException:
            # A native failure ends this session either way, and the service lock is held
            # for as long as the session lives. Let it escape and every later dictation
            # waits forever on a lock nobody will release.
            await self.close()
            raise

    def _feed_sync(self, content: bytes) -> dict[str, Any]:
        pcm = array.array("f")
        pcm.frombytes(content)
        if sys.byteorder == "big":
            pcm.byteswap()
        try:
            update = self._stream.feed(pcm)
            text = self._stream.text()
        except Exception as exc:
            raise AsrUnavailable(f"Nhận dạng giọng nói theo luồng thất bại: {exc}") from exc
        return self._event(update, text)

    async def finalize(self) -> dict[str, Any]:
        if self._closed:
            raise AsrUnavailable("Phiên nhận dạng giọng nói đã đóng")
        try:
            return await asyncio.to_thread(self._finalize_sync)
        finally:
            await self.close()

    def _finalize_sync(self) -> dict[str, Any]:
        try:
            update = self._stream.finalize()
            text = self._stream.text()
            snapshot = self._stream.snapshot()
        except Exception as exc:
            raise AsrUnavailable(f"Không thể kết thúc nhận dạng giọng nói: {exc}") from exc
        event = self._event(update, text)
        event.update(
            {
                "text": text.committed.strip() or text.display.strip(),
                "language": snapshot.language or self.language,
                "runtime": "transcribe.cpp-native",
            }
        )
        return event

    @staticmethod
    def _event(update: Any, text: Any) -> dict[str, Any]:
        return {
            "committed": text.committed,
            "tentative": text.tentative,
            "display": text.display,
            "revision": int(update.revision),
            "input_received_ms": int(update.input_received_ms),
            "audio_committed_ms": int(update.audio_committed_ms),
            "buffered_ms": int(update.buffered_ms),
            "result_changed": bool(update.result_changed),
            "committed_changed": bool(update.committed_changed),
            "tentative_changed": bool(update.tentative_changed),
        }

    async def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            await asyncio.to_thread(self._close_sync)
        finally:
            self._service._lock.release()

    def _close_sync(self) -> None:
        try:
            self._stream.reset()
        finally:
            self._session.close()


class AsrService:
    def __init__(
        self,
        *,
        data_dir: Path,
        executable: Path | None,
        model_path: Path,
        language: str,
        ffmpeg_executable: str = "",
        enabled: bool = True,
        gpu_leases: GpuLeaseManager | None = None,
        vram_reservation_bytes: int = 0,
    ) -> None:
        self.data_dir = data_dir
        self.configured_executable = executable
        self.model_path = model_path
        self.language = language
        self.ffmpeg_executable = ffmpeg_executable
        self.enabled = enabled
        self.gpu_leases = gpu_leases
        self.vram_reservation_bytes = vram_reservation_bytes
        self._lock = asyncio.Lock()
        self._native_binding: Any | None = None
        self._native_model: Any | None = None
        self._native_lease_owner = f"asr-native:{self.model_path.name}"

    def resolve_executable(self) -> Path | None:
        candidates: list[Path] = []
        if self.configured_executable:
            candidates.append(self.configured_executable.expanduser())
        source = self.data_dir / "source"
        candidates.extend(
            [
                source / "build" / "bin" / "transcribe-cli",
                source / "build" / "bin" / "Release" / "transcribe-cli.exe",
            ]
        )
        for name in ("transcribe-cli", "transcribe-cli.exe"):
            found = shutil.which(name)
            if found:
                candidates.append(Path(found))
        return next((path.resolve() for path in candidates if path.is_file()), None)

    def resolve_native_library(self) -> Path | None:
        source = self.data_dir / "source"
        override = os.environ.get("TRANSCRIBE_LIBRARY")
        candidates = [Path(override).expanduser()] if override else []
        candidates.extend(
            [
                source / "build-shared" / "src" / "libtranscribe.dylib",
                source / "build-shared" / "src" / "libtranscribe.so",
                source / "build-shared" / "bin" / "transcribe.dll",
                source / "build-shared" / "bin" / "Release" / "transcribe.dll",
                source / "build-shared" / "src" / "Release" / "transcribe.dll",
            ]
        )
        return next((path.resolve() for path in candidates if path.is_file()), None)

    def resolve_binding_source(self) -> Path | None:
        source = self.data_dir / "source" / "bindings" / "python" / "src"
        return source.resolve() if (source / "transcribe_cpp" / "__init__.py").is_file() else None

    def resolve_ffmpeg(self) -> str | None:
        if self.ffmpeg_executable:
            return self.ffmpeg_executable
        return shutil.which("ffmpeg") or (shutil.which("ffmpeg.exe") if os.name == "nt" else None)

    def _batch_available(self) -> bool:
        return bool(
            self.resolve_executable()
            and self.model_path.expanduser().is_file()
            and self.resolve_ffmpeg()
        )

    def _streaming_available(self) -> bool:
        return bool(
            self.resolve_native_library()
            and self.resolve_binding_source()
            and self.model_path.expanduser().is_file()
        )

    async def health(self) -> bool:
        return bool(self.enabled and (self._batch_available() or self._streaming_available()))

    def status(self) -> dict[str, Any]:
        executable = self.resolve_executable()
        native_library = self.resolve_native_library()
        binding_source = self.resolve_binding_source()
        ffmpeg = self.resolve_ffmpeg()
        model = self.model_path.expanduser()
        checksum_file = model.with_suffix(f"{model.suffix}.sha256")
        checksum = (
            checksum_file.read_text(encoding="ascii").strip() if checksum_file.is_file() else None
        )
        batch_available = bool(self.enabled and self._batch_available())
        streaming_available = bool(self.enabled and self._streaming_available())
        return {
            "available": batch_available or streaming_available,
            "batch_available": batch_available,
            "streaming_available": streaming_available,
            "native_model_loaded": self._native_model is not None,
            "language": self.language,
            "runtime": "transcribe.cpp",
            "executable": str(executable) if executable else None,
            "native_library": str(native_library) if native_library else None,
            "binding_source": str(binding_source) if binding_source else None,
            "model": str(model) if model.is_file() else None,
            "model_name": ASR_MODEL_NAME,
            "size_bytes": model.stat().st_size if model.is_file() else 0,
            "modified_at": model.stat().st_mtime if model.is_file() else None,
            "sha256": checksum,
            "ffmpeg": ffmpeg,
        }

    async def load(self) -> None:
        if not self.enabled or not self._streaming_available():
            raise AsrUnavailable("Chưa cài đặt nhận dạng giọng nói theo luồng")
        async with self._lock:
            await self._ensure_native_model()

    async def open_stream(self, *, language: str | None = None) -> AsrStream:
        if not self.enabled or not self._streaming_available():
            raise AsrUnavailable(
                "Chưa cài đặt nhận dạng giọng nói theo luồng. Hãy chạy private-ai-asr setup, "
                "rồi khởi động lại ứng dụng."
            )
        await self._lock.acquire()
        try:
            model = await self._ensure_native_model()
            session = await asyncio.to_thread(model.session)
            try:
                stream = await asyncio.to_thread(
                    session.stream,
                    language=language or self.language,
                )
            except Exception:
                await asyncio.to_thread(session.close)
                raise
            return AsrStream(self, session, stream, language or self.language)
        except Exception as exc:
            self._lock.release()
            if isinstance(exc, AsrUnavailable):
                raise
            raise AsrUnavailable(
                f"Không thể bắt đầu nhận dạng giọng nói theo luồng: {exc}"
            ) from exc

    async def _ensure_native_model(self) -> Any:
        if self._native_model is not None:
            return self._native_model
        reserved = False
        try:
            if self.gpu_leases:
                await self.gpu_leases.reserve(
                    self._native_lease_owner,
                    self.vram_reservation_bytes,
                )
                reserved = True
            binding = await asyncio.to_thread(self._load_binding_sync)
            model = await asyncio.to_thread(
                binding.Model,
                str(self.model_path.expanduser().resolve()),
            )
            capabilities = await asyncio.to_thread(lambda: model.capabilities)
            if not capabilities.supports_streaming:
                await asyncio.to_thread(model.close)
                raise AsrUnavailable("Mô hình ASR đang dùng không hỗ trợ chế độ luồng")
            self._native_model = model
            return model
        except Exception:
            if reserved and self.gpu_leases:
                await self.gpu_leases.release(self._native_lease_owner)
            raise

    def _load_binding_sync(self) -> Any:
        if self._native_binding is not None:
            return self._native_binding
        library = self.resolve_native_library()
        binding_source = self.resolve_binding_source()
        if not library or not binding_source:
            raise AsrUnavailable("Thiếu thư viện native của transcribe.cpp")
        os.environ["TRANSCRIBE_LIBRARY"] = str(library)
        source_value = str(binding_source)
        if source_value not in sys.path:
            sys.path.insert(0, source_value)
        self._native_binding = importlib.import_module("transcribe_cpp")
        return self._native_binding

    async def close(self) -> None:
        """Release the model. Bounded: shutdown must not hang behind a live session.

        The lock is held for the whole of a streaming session, so quitting mid-dictation
        would otherwise block ``close_services`` indefinitely. Past the deadline the model
        goes anyway — the process is on its way out and the VRAM reservation with it.
        """
        acquired = False
        try:
            await asyncio.wait_for(self._lock.acquire(), CLOSE_TIMEOUT_SECONDS)
            acquired = True
        except TimeoutError:
            logger.warning("Đóng ASR khi phiên nhận dạng chưa trả khoá")
        try:
            model, self._native_model = self._native_model, None
            if model is not None:
                await asyncio.to_thread(model.close)
            if self.gpu_leases:
                await self.gpu_leases.release(self._native_lease_owner)
        finally:
            if acquired:
                self._lock.release()

    async def transcribe(
        self,
        content: bytes,
        *,
        filename: str = "recording.webm",
        language: str | None = None,
    ) -> dict[str, Any]:
        if not content:
            raise ValueError("Dữ liệu âm thanh không được để trống")
        async with self._lock:
            if self.gpu_leases:
                owner = f"asr-batch:{self.model_path.name}"
                async with self.gpu_leases.temporary(
                    owner,
                    self.vram_reservation_bytes,
                ):
                    return await asyncio.to_thread(
                        self._transcribe_sync,
                        content,
                        filename,
                        language or self.language,
                    )
            return await asyncio.to_thread(
                self._transcribe_sync,
                content,
                filename,
                language or self.language,
            )

    def _transcribe_sync(self, content: bytes, filename: str, language: str) -> dict[str, Any]:
        executable = self.resolve_executable()
        ffmpeg = self.resolve_ffmpeg()
        model = self.model_path.expanduser().resolve()
        if not self.enabled or not executable or not model.is_file() or not ffmpeg:
            raise AsrUnavailable(
                "Chưa cài đặt ASR. Hãy chạy private-ai-asr setup, rồi khởi động lại ứng dụng."
            )
        suffix = Path(filename).suffix.lower()
        if not re.fullmatch(r"\.[a-z0-9]{1,8}", suffix):
            suffix = ".audio"
        self.data_dir.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="private-ai-asr-", dir=self.data_dir) as temp:
            temp_dir = Path(temp)
            source = temp_dir / f"input{suffix}"
            normalized = temp_dir / "audio.wav"
            manifest = temp_dir / "files.txt"
            source.write_bytes(content)
            conversion = subprocess.run(  # noqa: S603
                [
                    ffmpeg,
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-i",
                    str(source),
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                    str(normalized),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            if conversion.returncode != 0:
                raise ValueError(conversion.stderr.strip() or "FFmpeg không giải mã được âm thanh")
            manifest.write_text(f"{normalized}\n", encoding="utf-8")
            completed = subprocess.run(  # noqa: S603
                [
                    str(executable),
                    "-q",
                    "-m",
                    str(model),
                    "--language",
                    language,
                    "--batch",
                    str(manifest),
                    "--batch-jsonl",
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                detail = completed.stderr.strip() or completed.stdout.strip()
                raise AsrUnavailable(detail or "transcribe.cpp thất bại")
            result = self._parse_result(completed.stdout)
            result.update({"language": language, "runtime": "transcribe.cpp"})
            return result

    @staticmethod
    def _parse_result(output: str) -> dict[str, Any]:
        for line in reversed(output.splitlines()):
            try:
                parsed = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict) and "text" in parsed:
                return parsed
        match = re.search(r"(?m)^text:\s*(.+)$", output)
        if match:
            return {"text": match.group(1).strip()}
        raise AsrUnavailable("transcribe.cpp không trả về bản ghi nào")
