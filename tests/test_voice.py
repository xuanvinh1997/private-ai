"""Dictation: the lifecycle, and the two ways it used to fail silently.

Every failure this file guards was reachable from the microphone button. A native error
mid-stream left ``AsrService._lock`` held, so the *next* click hung forever with nothing on
screen; a slow ``open_stream`` left the capture in ``idle``, so a second click opened a
second session on top of the first. Both look identical from the outside — a button that
does nothing — which is why they are asserted here rather than left to a manual try.

Nothing below touches a real microphone or the native library: the device, the permission
prompt and the recogniser are all stubbed, and what is under test is the state machine
between them.
"""

from __future__ import annotations

import array
import asyncio
from pathlib import Path
from typing import Any

import pytest

from private_ai.asr.service import AsrService, AsrUnavailable
from private_ai.ui.audio import capture as cap

pytestmark = pytest.mark.usefixtures("qapp")


# --- stubs ----------------------------------------------------------------


class FakeText:
    def __init__(self, committed: str = "", tentative: str = "") -> None:
        self.committed = committed
        self.tentative = tentative
        self.display = (committed + tentative).strip()


class FakeUpdate:
    def __init__(self, changed: bool) -> None:
        self.revision = 1
        self.input_received_ms = 320
        self.audio_committed_ms = 320
        self.buffered_ms = 0
        self.result_changed = changed
        self.committed_changed = changed
        self.tentative_changed = changed


class FakeSnapshot:
    language = "vi-VN"


class FakeNativeStream:
    """The transcribe.cpp streaming handle, reduced to what ``AsrStream`` calls."""

    def __init__(self, *, transcript: str = "xin chào", feed_error: bool = False) -> None:
        self.transcript = transcript
        self.feed_error = feed_error
        self.fed = 0
        self.reset_calls = 0

    def feed(self, _pcm: Any) -> FakeUpdate:
        if self.feed_error:
            raise RuntimeError("native feed exploded")
        self.fed += 1
        return FakeUpdate(changed=True)

    def text(self) -> FakeText:
        return FakeText(self.transcript if self.fed else "")

    def finalize(self) -> FakeUpdate:
        return FakeUpdate(changed=False)

    def snapshot(self) -> FakeSnapshot:
        return FakeSnapshot()

    def reset(self) -> None:
        self.reset_calls += 1


class FakeSession:
    def __init__(self, stream: FakeNativeStream) -> None:
        self._stream = stream
        self.closed = False

    def stream(self, *, language: str = "") -> FakeNativeStream:  # noqa: ARG002
        return self._stream

    def close(self) -> None:
        self.closed = True


class FakeModel:
    def __init__(self, stream: FakeNativeStream) -> None:
        self._stream = stream
        self.sessions: list[FakeSession] = []
        self.closed = False

    def session(self) -> FakeSession:
        session = FakeSession(self._stream)
        self.sessions.append(session)
        return session

    def close(self) -> None:
        self.closed = True


def build_service(tmp_path: Path, native: FakeNativeStream) -> tuple[AsrService, FakeModel]:
    """A real ``AsrService`` with only the native model swapped out."""
    service = AsrService(
        data_dir=tmp_path,
        executable=None,
        model_path=tmp_path / "model.gguf",
        language="vi-VN",
        enabled=True,
    )
    model = FakeModel(native)
    service._streaming_available = lambda: True  # type: ignore[method-assign]
    service._batch_available = lambda: True  # type: ignore[method-assign]
    # Pre-loaded, exactly as a second dictation in the same run would find it, so
    # ``_ensure_native_model`` stays the real one.
    service._native_model = model
    return service, model


FRAME = array.array("f", [0.0] * cap.FRAME_SAMPLES).tobytes()


# --- the lock, which is what "the mic stopped working" actually was --------


async def test_feed_failure_releases_the_service_lock(tmp_path: Path) -> None:
    """A native error must end the session, not park the lock for the rest of the run."""
    service, _ = build_service(tmp_path, FakeNativeStream(feed_error=True))
    stream = await service.open_stream()
    assert service._lock.locked()

    with pytest.raises(AsrUnavailable):
        await stream.feed(FRAME)

    assert not service._lock.locked()
    # The proof that matters: dictation still works afterwards.
    second = await asyncio.wait_for(service.open_stream(), timeout=2)
    await second.close()


async def test_finalize_releases_the_service_lock(tmp_path: Path) -> None:
    service, _ = build_service(tmp_path, FakeNativeStream())
    stream = await service.open_stream()
    await stream.feed(FRAME)
    result = await stream.finalize()

    assert result["text"] == "xin chào"
    assert not service._lock.locked()


async def test_close_does_not_hang_behind_a_live_session(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Quitting mid-dictation used to block ``close_services`` forever."""
    monkeypatch.setattr("private_ai.asr.service.CLOSE_TIMEOUT_SECONDS", 0.05)
    service, model = build_service(tmp_path, FakeNativeStream())
    await service.open_stream()  # deliberately left open

    await asyncio.wait_for(service.close(), timeout=2)
    assert model.closed


# --- the capture state machine -------------------------------------------


class FakeDevice:
    @staticmethod
    def isNull() -> bool:  # noqa: N802 - Qt naming
        return False


class FakeMediaDevices:
    @staticmethod
    def defaultAudioInput() -> FakeDevice:  # noqa: N802 - Qt naming
        return FakeDevice()


class Services:
    def __init__(self, asr: Any) -> None:
        self.asr = asr


@pytest.fixture
def microphone(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """A ``MicrophoneCapture`` wired to a stub recogniser and no audio hardware."""
    monkeypatch.setattr(cap, "MULTIMEDIA_AVAILABLE", True)
    monkeypatch.setattr(cap, "QMediaDevices", FakeMediaDevices, raising=False)

    def build(native: FakeNativeStream | None = None, service: Any = None):
        if service is None:
            service, _ = build_service(tmp_path, native or FakeNativeStream())
        capture = cap.MicrophoneCapture(Services(service), language="vi-VN")

        async def granted() -> bool:
            return True

        capture._request_permission = granted  # type: ignore[method-assign]
        capture._open_input = lambda _device: True  # type: ignore[method-assign]
        return capture, service

    return build


def record(capture: cap.MicrophoneCapture) -> dict[str, list[Any]]:
    seen: dict[str, list[Any]] = {"state": [], "text": [], "notice": [], "error": [], "level": []}
    capture.stateChanged.connect(seen["state"].append)
    capture.transcriptChanged.connect(seen["text"].append)
    capture.notice.connect(seen["notice"].append)
    capture.failed.connect(seen["error"].append)
    capture.levelChanged.connect(seen["level"].append)
    return seen


async def test_preparing_precedes_recording(microphone) -> None:
    """The wait for the model is a state of its own, or the click reads as ignored."""
    capture, _ = microphone()
    seen = record(capture)

    await capture.start()

    assert seen["state"] == [cap.STATE_PREPARING, cap.STATE_RECORDING]
    assert capture.is_busy()
    capture.cancel()


async def test_a_second_click_while_preparing_opens_no_second_session(
    tmp_path: Path,
    microphone,
) -> None:
    service, _ = build_service(tmp_path, FakeNativeStream())
    opened = 0
    original = service.open_stream

    async def slow(**kwargs: Any):
        nonlocal opened
        opened += 1
        await asyncio.sleep(0.05)
        return await original(**kwargs)

    service.open_stream = slow  # type: ignore[method-assign]
    capture, _ = microphone(service=service)

    first = asyncio.create_task(capture.start())
    await asyncio.sleep(0.01)
    assert capture.state() == cap.STATE_PREPARING
    await capture.start()  # the second click, mid-load
    await first

    assert opened == 1
    assert capture.state() == cap.STATE_RECORDING
    capture.cancel()


async def test_cancel_while_preparing_gives_the_lock_back(tmp_path: Path, microphone) -> None:
    """Leaving the view mid-load must not strand the session that is still arriving."""
    service, _ = build_service(tmp_path, FakeNativeStream())
    original = service.open_stream

    async def slow(**kwargs: Any):
        await asyncio.sleep(0.05)
        return await original(**kwargs)

    service.open_stream = slow  # type: ignore[method-assign]
    capture, _ = microphone(service=service)

    task = asyncio.create_task(capture.start())
    await asyncio.sleep(0.01)
    capture.cancel()
    await task

    assert capture.state() == cap.STATE_IDLE
    assert not service._lock.locked()


async def test_stop_finalizes_and_writes_the_transcript(microphone) -> None:
    capture, _ = microphone(FakeNativeStream(transcript="xin chào"))
    seen = record(capture)
    await capture.start("Ghi chú")

    capture._dispatch(FRAME)
    capture.stop()
    for _ in range(50):
        await asyncio.sleep(0.01)
        if capture.state() == cap.STATE_IDLE:
            break

    assert seen["state"] == [
        cap.STATE_PREPARING,
        cap.STATE_RECORDING,
        cap.STATE_TRANSCRIBING,
        cap.STATE_IDLE,
    ]
    assert seen["text"][-1] == "Ghi chú xin chào"
    assert not seen["error"]


async def test_a_silent_session_says_so_instead_of_nothing(microphone) -> None:
    """An empty transcript rewrote the draft with itself and looked like a broken feature."""
    capture, _ = microphone(FakeNativeStream(transcript=""))
    seen = record(capture)
    await capture.start()

    capture._dispatch(FRAME)
    capture.stop()
    for _ in range(50):
        await asyncio.sleep(0.01)
        if capture.state() == cap.STATE_IDLE:
            break

    assert seen["notice"] == [cap.NO_SOUND]


async def test_feed_failure_reports_and_frees_the_recogniser(tmp_path: Path, microphone) -> None:
    service, _ = build_service(tmp_path, FakeNativeStream(feed_error=True))
    capture, _ = microphone(service=service)
    seen = record(capture)
    await capture.start()

    capture._dispatch(FRAME)
    for _ in range(50):
        await asyncio.sleep(0.01)
        if capture.state() == cap.STATE_IDLE:
            break

    assert seen["error"], "a native failure has to reach the user"
    assert capture.state() == cap.STATE_IDLE
    assert not service._lock.locked()


async def test_batch_fallback_is_announced(tmp_path: Path, microphone) -> None:
    """No partial text will appear at all, so the silence needs explaining."""
    service, _ = build_service(tmp_path, FakeNativeStream())

    async def unavailable(**_kwargs: Any):
        raise AsrUnavailable("VRAM đã dùng hết")

    service.open_stream = unavailable  # type: ignore[method-assign]
    capture, _ = microphone(service=service)
    seen = record(capture)

    await capture.start()

    assert seen["notice"] == [cap.BATCH_FALLBACK]
    assert capture.state() == cap.STATE_RECORDING
    capture.cancel()


async def test_level_is_published_for_the_meter(microphone) -> None:
    capture, _ = microphone()
    seen = record(capture)
    capture._resampler = cap._Resampler(cap.TARGET_SAMPLE_RATE)
    capture._decode = lambda block: [0.5] * (len(block) // 4)
    capture._bytes_per_frame = 4

    capture._ingest(b"\x00\x00\x00\x00" * 256)

    assert seen["level"], "the meter has no other source of motion"
    assert seen["level"][-1] > 0.5


# --- the widget -----------------------------------------------------------


def test_meter_shows_only_while_dictation_runs() -> None:
    from private_ai.ui.widgets.voice_meter import VoiceMeter

    meter = VoiceMeter()
    assert not meter.isVisibleTo(meter)

    meter.set_state(cap.STATE_PREPARING)
    assert meter.isVisibleTo(meter)
    assert meter.toolTip()
    assert meter.accessibleName()

    meter.set_state(cap.STATE_IDLE)
    assert not meter.isVisibleTo(meter)
    assert not meter._timer.isActive()
