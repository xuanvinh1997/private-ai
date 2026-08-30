"""Microphone capture, replacing the browser's AudioWorklet and its WebSocket.

The transport is gone — frames go straight into ``AsrStream.feed`` — but the framing it
carried is not negotiable: the model is fed **float32 mono 16 kHz in 5120-sample
(320 ms) frames**, and a flush drains the tail before finalizing, or the last third of a
second of speech is simply lost.

Qt hands us whatever the device actually supports, which on most machines is 44.1 or
48 kHz and often int16. So the resampling the worklet used to do is done here, in the
same way it did it: linear interpolation between neighbouring input samples, with a
fractional read cursor advanced by ``source_rate / 16000`` per output sample. Keeping
that identical matters because it is what the model has heard for every recording made
so far.
"""

from __future__ import annotations

import array
import asyncio
import contextlib
import io
import logging
import sys
import wave
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QCoreApplication, QObject, Signal

from private_ai.asr.service import AsrUnavailable
from private_ai.ui.async_bridge import run_coro

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Callable, Iterable, Sequence

    from private_ai.core.services import AppServices

__all__ = [
    "FRAME_SAMPLES",
    "MULTIMEDIA_AVAILABLE",
    "STATE_IDLE",
    "STATE_RECORDING",
    "STATE_TRANSCRIBING",
    "TARGET_SAMPLE_RATE",
    "MicrophoneCapture",
]

logger = logging.getLogger(__name__)

# QtMultimedia ships in PySide6-Addons, not PySide6-Essentials. An install without it
# must still let the chat view import and run — only the microphone button goes dark.
try:
    from PySide6.QtMultimedia import QAudioFormat, QAudioSource, QMediaDevices

    MULTIMEDIA_AVAILABLE = True
except ImportError:  # pragma: no cover - depends on how PySide6 was installed
    QAudioFormat = QAudioSource = QMediaDevices = None  # type: ignore[assignment,misc]
    MULTIMEDIA_AVAILABLE = False

TARGET_SAMPLE_RATE = 16_000
FRAME_SAMPLES = 5_120  # 320 ms, what the ASR model expects per feed

STATE_IDLE = "idle"
STATE_RECORDING = "recording"
STATE_TRANSCRIBING = "transcribing"

NO_MULTIMEDIA = "Thiếu QtMultimedia — cài PySide6-Addons để dùng micro"
NO_MICROPHONE = "Không tìm thấy microphone nào trên máy"
NO_PERMISSION = "Ứng dụng chưa được cấp quyền dùng microphone"
NO_FORMAT = "Thiết bị thu âm không hỗ trợ định dạng nào dùng được"
NO_ASR = "Nhận dạng giọng nói chưa sẵn sàng"
CAPTURE_FAILED = "Không thể mở microphone"


def _to_little_endian(samples: array.array) -> bytes:
    """AsrStream reads float32 little-endian; ``array`` writes native order."""
    if sys.byteorder == "big":
        copy = array.array("f", samples)
        copy.byteswap()
        return copy.tobytes()
    return samples.tobytes()


class _Resampler:
    """Linear-interpolation resampler to 16 kHz, emitting fixed 5120-sample frames.

    Ported from ``pcm-worklet.js`` verbatim, including the fractional read cursor and the
    consume-then-trim of the input buffer, so the sample stream is bit-comparable.
    """

    def __init__(self, source_rate: int, *, chunk: int = FRAME_SAMPLES) -> None:
        self.ratio = float(source_rate) / float(TARGET_SAMPLE_RATE)
        self.chunk = chunk
        self._source: list[float] = []
        self._read = 0.0
        self._output = array.array("f")

    def push(self, samples: Sequence[float]) -> list[bytes]:
        if not samples:
            return []
        frames: list[bytes] = []
        source = self._source
        source.extend(samples)
        ratio = self.ratio
        output = self._output
        while self._read + 1 < len(source):
            index = int(self._read)
            fraction = self._read - index
            value = source[index] * (1.0 - fraction) + source[index + 1] * fraction
            output.append(max(-1.0, min(1.0, value)))
            self._read += ratio
            if len(output) >= self.chunk:
                frames.extend(self._drain(flush=False))
        consumed = int(self._read)
        if consumed > 0:
            del source[:consumed]
            self._read -= consumed
        return frames

    def flush(self) -> list[bytes]:
        """The tail, however short — a partial frame still carries words."""
        return self._drain(flush=True)

    def _drain(self, *, flush: bool) -> list[bytes]:
        frames: list[bytes] = []
        output = self._output
        while len(output) >= self.chunk or (flush and output):
            size = min(self.chunk, len(output))
            frames.append(_to_little_endian(output[:size]))
            del output[:size]
        return frames


def _decoder(sample_format: Any, channels: int) -> tuple[Callable[[bytes], list[float]], bool]:
    """Build a native-endian PCM decoder that also downmixes to mono.

    Returns ``(decode, supported)``; an unsupported sample format is a hard stop rather
    than silently feeding the model noise.
    """
    fmt = QAudioFormat.SampleFormat
    if sample_format == fmt.Float:
        typecode, scale, offset = "f", 1.0, 0.0
    elif sample_format == fmt.Int16:
        typecode, scale, offset = "h", 1.0 / 32768.0, 0.0
    elif sample_format == fmt.Int32:
        typecode, scale, offset = "i", 1.0 / 2147483648.0, 0.0
    elif sample_format == fmt.UInt8:
        typecode, scale, offset = "B", 1.0 / 128.0, -128.0
    else:
        return (lambda _block: []), False

    def decode(block: bytes) -> list[float]:
        values = array.array(typecode)
        values.frombytes(block)
        if channels <= 1:
            return [(value + offset) * scale for value in values]
        mixed: list[float] = []
        for start in range(0, len(values) - channels + 1, channels):
            total = 0.0
            for step in range(channels):
                total += (values[start + step] + offset) * scale
            mixed.append(total / channels)
        return mixed

    return decode, True


def _wav_bytes(pcm: bytes) -> bytes:
    """Wrap accumulated float32 frames as 16-bit PCM WAV for the batch transcriber."""
    samples = array.array("f")
    samples.frombytes(pcm)
    if sys.byteorder == "big":
        samples.byteswap()
    scaled = array.array("h", (int(max(-1.0, min(1.0, value)) * 32767) for value in samples))
    if sys.byteorder == "big":
        scaled.byteswap()
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as writer:
        writer.setnchannels(1)
        writer.setsampwidth(2)
        writer.setframerate(TARGET_SAMPLE_RATE)
        writer.writeframes(scaled.tobytes())
    return buffer.getvalue()


class MicrophoneCapture(QObject):
    """One dictation session: mic in, merged composer text out.

    The draft the user had already typed is captured at record start and every result is
    appended to it, so speaking never erases what was typed — the web app's
    ``voiceDraftBase`` rule, kept.
    """

    transcriptChanged = Signal(str)  # merged draft, safe to write into the composer
    transcriptFinal = Signal(str)
    stateChanged = Signal(str)  # idle | recording | transcribing
    failed = Signal(str)

    def __init__(
        self,
        services: AppServices,
        *,
        language: str = "vi-VN",
        parent: QObject | None = None,
    ) -> None:
        super().__init__(parent)
        self._services = services
        self._language = language
        self._state = STATE_IDLE
        self._draft_base = ""
        self._streaming = False

        self._source: QAudioSource | None = None
        self._io_device: Any = None
        self._resampler: _Resampler | None = None
        self._decode: Callable[[bytes], list[float]] = lambda _block: []
        self._bytes_per_frame = 2
        self._leftover = bytearray()

        self._stream: Any = None
        self._queue: asyncio.Queue[bytes | None] | None = None
        self._worker: asyncio.Task[None] | None = None
        self._batch = bytearray()

    # --- state ------------------------------------------------------------

    @staticmethod
    def available() -> bool:
        """False on a PySide6-Essentials-only install; the button should stay disabled."""
        return MULTIMEDIA_AVAILABLE

    def state(self) -> str:
        return self._state

    def is_recording(self) -> bool:
        return self._state == STATE_RECORDING

    def is_busy(self) -> bool:
        return self._state != STATE_IDLE

    def _set_state(self, state: str) -> None:
        if state == self._state:
            return
        self._state = state
        self.stateChanged.emit(state)

    def _fail(self, message: str) -> None:
        self.failed.emit(message)
        self._set_state(STATE_IDLE)

    # --- session ----------------------------------------------------------

    async def start(self, draft_base: str = "") -> None:
        """Open the recogniser, then the microphone. Never raises; reports via ``failed``."""
        if self._state != STATE_IDLE:
            return
        if not MULTIMEDIA_AVAILABLE:
            self._fail(NO_MULTIMEDIA)
            return
        self._draft_base = draft_base.rstrip()
        self._batch.clear()
        self._leftover.clear()

        device = QMediaDevices.defaultAudioInput()
        if device.isNull():
            self._fail(NO_MICROPHONE)
            return
        if not await self._request_permission():
            self._fail(NO_PERMISSION)
            return

        # The recogniser is opened first: a failure here should not leave the mic on, and
        # it is also what decides between the streaming and the record-then-send path.
        try:
            self._stream = await self._services.asr.open_stream(language=self._language)
            self._streaming = True
        except AsrUnavailable:
            self._stream = None
            self._streaming = False
            if not await self._services.asr.health():
                self._fail(NO_ASR)
                return
        except Exception as exc:  # a broken recogniser must not take the app with it
            logger.exception("Không mở được phiên nhận dạng giọng nói")
            self._fail(f"{NO_ASR}: {exc}")
            return

        if not self._open_input(device):
            await self._discard_stream()
            return

        if self._streaming:
            self._queue = asyncio.Queue()
            self._worker = run_coro(self._pump(), owner=self, on_error=self._on_worker_error)
        self._set_state(STATE_RECORDING)

    def stop(self) -> None:
        """Flush the tail and finalize. The partial text already emitted is kept."""
        if self._state != STATE_RECORDING:
            return
        self._set_state(STATE_TRANSCRIBING)
        self._close_input()
        if self._streaming:
            if self._queue is None:
                # The pump already exited — nothing is left to finalize.
                self._set_state(STATE_IDLE)
                return
            self._queue.put_nowait(None)
            return
        run_coro(self._finish_batch(), owner=self, on_error=self._on_worker_error)

    def cancel(self) -> None:
        """Abandon the session without producing a transcript."""
        if self._state == STATE_IDLE:
            return
        self._close_input()
        self._batch.clear()
        worker, self._worker = self._worker, None
        if worker is not None and not worker.done():
            worker.cancel()
        elif self._stream is not None:
            run_coro(self._discard_stream(), owner=self)
        self._queue = None
        self._set_state(STATE_IDLE)

    # --- audio device -----------------------------------------------------

    def _open_input(self, device: Any) -> bool:
        audio_format = self._negotiate(device)
        if audio_format is None:
            self._fail(NO_FORMAT)
            return False
        decode, supported = _decoder(audio_format.sampleFormat(), audio_format.channelCount())
        if not supported:
            self._fail(NO_FORMAT)
            return False

        self._decode = decode
        self._bytes_per_frame = max(1, int(audio_format.bytesPerFrame()))
        self._resampler = _Resampler(int(audio_format.sampleRate()))
        source = QAudioSource(device, audio_format, self)
        # Pull mode: QAudioSource hands back a QIODevice that buffers until we read it.
        io_device = source.start()
        if io_device is None:
            source.deleteLater()
            self._fail(CAPTURE_FAILED)
            return False
        io_device.readyRead.connect(self._on_ready_read)
        self._source = source
        self._io_device = io_device
        return True

    @staticmethod
    def _negotiate(device: Any) -> QAudioFormat | None:
        """Ask for what we want, settle for what the device has — we convert either way."""
        preferred = device.preferredFormat()
        rate = int(preferred.sampleRate()) or 48_000
        formats = QAudioFormat.SampleFormat
        candidates: list[QAudioFormat] = []
        for sample_rate in (TARGET_SAMPLE_RATE, rate):
            for sample_format in (formats.Float, formats.Int16, formats.Int32, formats.UInt8):
                candidate = QAudioFormat()
                candidate.setSampleRate(sample_rate)
                candidate.setChannelCount(1)
                candidate.setSampleFormat(sample_format)
                candidates.append(candidate)
        candidates.append(preferred)
        return next(
            (
                candidate
                for candidate in candidates
                if candidate.isValid() and device.isFormatSupported(candidate)
            ),
            None,
        )

    def _on_ready_read(self) -> None:
        device = self._io_device
        if device is None:
            return
        payload = bytes(device.readAll().data())
        if payload:
            self._ingest(payload)

    def _ingest(self, payload: bytes) -> None:
        resampler = self._resampler
        if resampler is None:
            return
        buffer = self._leftover
        buffer.extend(payload)
        stride = self._bytes_per_frame
        usable = len(buffer) - (len(buffer) % stride)
        if usable <= 0:
            return
        block = bytes(buffer[:usable])
        del buffer[:usable]
        for frame in resampler.push(self._decode(block)):
            self._dispatch(frame)

    def _dispatch(self, frame: bytes) -> None:
        if self._streaming and self._queue is not None:
            self._queue.put_nowait(frame)
        else:
            self._batch.extend(frame)

    def _close_input(self) -> None:
        """Stop the device, then push the resampler's tail through — order matters."""
        source, self._source = self._source, None
        device = self._io_device
        if device is not None:
            with contextlib.suppress(RuntimeError, TypeError):
                device.readyRead.disconnect(self._on_ready_read)
            with contextlib.suppress(RuntimeError):
                remaining = bytes(device.readAll().data())
                if remaining:
                    self._ingest(remaining)
        self._io_device = None
        if source is not None:
            source.stop()
            source.deleteLater()
        resampler, self._resampler = self._resampler, None
        if resampler is not None:
            for frame in resampler.flush():
                self._dispatch(frame)
        self._leftover.clear()

    # --- recognition ------------------------------------------------------

    async def _pump(self) -> None:
        stream = self._stream
        queue = self._queue
        if stream is None or queue is None:
            return
        try:
            while True:
                frame = await queue.get()
                if frame is None:
                    break
                event = await stream.feed(frame)
                if event.get("result_changed"):
                    self._emit_partial(str(event.get("display") or ""))
            result = await stream.finalize()
            self._emit_final(str(result.get("text") or result.get("display") or ""))
        except asyncio.CancelledError:
            with contextlib.suppress(Exception):
                await stream.close()
            raise
        finally:
            self._stream = None
            self._queue = None
            self._worker = None

    async def _finish_batch(self) -> None:
        audio, self._batch = bytes(self._batch), bytearray()
        if not audio:
            self._set_state(STATE_IDLE)
            return
        result = await self._services.asr.transcribe(
            _wav_bytes(audio),
            filename="recording.wav",
            language=self._language,
        )
        self._emit_final(str(result.get("text") or ""))

    async def _discard_stream(self) -> None:
        stream, self._stream = self._stream, None
        if stream is not None:
            with contextlib.suppress(Exception):
                await stream.close()

    def _on_worker_error(self, exc: BaseException) -> None:
        logger.exception("Nhận dạng giọng nói thất bại", exc_info=exc)
        self._stream = None
        self._queue = None
        self._worker = None
        self._batch.clear()
        self._fail(str(exc) or NO_ASR)

    # --- text -------------------------------------------------------------

    def _merge(self, text: str) -> str:
        base = self._draft_base
        cleaned = text.strip()
        separator = " " if base and cleaned else ""
        return f"{base}{separator}{cleaned}"

    def _emit_partial(self, text: str) -> None:
        self.transcriptChanged.emit(self._merge(text))

    def _emit_final(self, text: str) -> None:
        merged = self._merge(text)
        self.transcriptChanged.emit(merged)
        self.transcriptFinal.emit(merged)
        self._set_state(STATE_IDLE)

    # --- permissions ------------------------------------------------------

    async def _request_permission(self) -> bool:
        """Best effort: macOS gates the mic behind this, other platforms have no API."""
        try:
            from PySide6.QtCore import QMicrophonePermission, Qt
        except ImportError:  # pragma: no cover - older Qt has no permission API
            return True
        app = QCoreApplication.instance()
        if app is None or not hasattr(app, "checkPermission"):
            return True
        try:
            permission = QMicrophonePermission()
            granted = Qt.PermissionStatus.Granted
            status = app.checkPermission(permission)
            if status == granted:
                return True
            if status == Qt.PermissionStatus.Denied:
                return False
            future: asyncio.Future[bool] = asyncio.get_running_loop().create_future()

            def answered(result: Any) -> None:
                if not future.done():
                    future.set_result(app.checkPermission(result) == granted)

            app.requestPermission(permission, answered)
            return await future
        except Exception:  # pragma: no cover - platform quirk, never fatal
            logger.debug("Không kiểm tra được quyền microphone", exc_info=True)
            return True


def frames_from(samples: Iterable[float], source_rate: int) -> list[bytes]:
    """Resample one buffer to the wire framing. Exposed for tests, not used by the view."""
    resampler = _Resampler(source_rate)
    frames = resampler.push(list(samples))
    frames.extend(resampler.flush())
    return frames
