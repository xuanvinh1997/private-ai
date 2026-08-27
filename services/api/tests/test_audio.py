from __future__ import annotations

import array
from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient

from private_ai_api.services.asr import AsrService, AsrStream
from private_ai_api.services.gpu_lease import GpuLeaseManager


def test_asr_parses_jsonl_result() -> None:
    result = AsrService._parse_result(
        'diagnostic output\n{"file":"audio.wav","text":"Xin chào thế giới."}\n'
    )
    assert result["text"] == "Xin chào thế giới."


def test_audio_endpoint_returns_transcript(client: TestClient, monkeypatch) -> None:
    async def fake_transcribe(
        content: bytes,
        *,
        filename: str,
        language: str | None,
    ) -> dict[str, object]:
        assert content == b"fake-audio"
        assert filename == "voice.webm"
        return {"text": "Nội dung giọng nói", "language": language or "vi-VN"}

    monkeypatch.setattr(client.app.state.services.asr, "transcribe", fake_transcribe)
    response = client.post(
        "/api/v1/asr/transcribe",
        files={"file": ("voice.webm", b"fake-audio", "audio/webm")},
    )
    assert response.status_code == 200
    assert response.json()["text"] == "Nội dung giọng nói"


def test_audio_websocket_accepts_binary_chunks(client: TestClient, monkeypatch) -> None:
    async def fake_transcribe(
        content: bytes,
        *,
        filename: str,
        language: str | None,
    ) -> dict[str, object]:
        assert content == b"chunk-onechunk-two"
        assert filename == "voice.webm"
        return {"text": "Xin chào", "language": language or "vi-VN"}

    monkeypatch.setattr(client.app.state.services.asr, "transcribe", fake_transcribe)
    with client.websocket_connect("/api/v1/asr/stream") as websocket:
        assert websocket.receive_json()["type"] == "ready"
        websocket.send_json(
            {"type": "config", "language": "vi-VN", "filename": "voice.webm"}
        )
        websocket.send_bytes(b"chunk-one")
        assert websocket.receive_json() == {"type": "progress", "bytes": 9}
        websocket.send_bytes(b"chunk-two")
        assert websocket.receive_json() == {"type": "progress", "bytes": 18}
        websocket.send_json({"type": "commit"})
        result = websocket.receive_json()
        assert result["type"] == "final"
        assert result["text"] == "Xin chào"


def test_audio_websocket_streams_native_pcm_partials(
    client: TestClient,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeNativeStream:
        async def feed(self, content: bytes) -> dict[str, object]:
            assert content == b"\x00\x00\x00\x00"
            return {
                "display": "Xin chào",
                "committed": "Xin ",
                "tentative": "chào",
                "revision": 1,
                "input_received_ms": 320,
                "audio_committed_ms": 160,
                "buffered_ms": 160,
                "result_changed": True,
                "committed_changed": True,
                "tentative_changed": True,
            }

        async def finalize(self) -> dict[str, object]:
            return {"text": "Xin chào bạn", "language": "vi-VN"}

        async def close(self) -> None:
            raise AssertionError("finalized streams must not be closed twice by the router")

    async def fake_open_stream(*, language: str | None = None) -> FakeNativeStream:
        assert language == "vi-VN"
        return FakeNativeStream()

    monkeypatch.setattr(client.app.state.services.asr, "open_stream", fake_open_stream)
    monkeypatch.setattr(
        client.app.state.services.asr,
        "status",
        lambda: {"streaming_available": True},
    )
    with client.websocket_connect("/api/v1/asr/stream") as websocket:
        ready = websocket.receive_json()
        assert ready["type"] == "ready"
        assert ready["streaming"] is True
        websocket.send_json(
            {"type": "config", "language": "vi-VN", "format": "f32le", "sample_rate": 16000}
        )
        assert websocket.receive_json()["type"] == "configured"
        websocket.send_bytes(b"\x00\x00\x00\x00")
        partial = websocket.receive_json()
        assert partial["type"] == "partial"
        assert partial["display"] == "Xin chào"
        websocket.send_json({"type": "commit"})
        final = websocket.receive_json()
        assert final["type"] == "final"
        assert final["text"] == "Xin chào bạn"


@dataclass
class FakeUpdate:
    revision: int = 2
    input_received_ms: int = 320
    audio_committed_ms: int = 200
    buffered_ms: int = 120
    result_changed: bool = True
    committed_changed: bool = True
    tentative_changed: bool = True


class FakeBindingStream:
    def __init__(self) -> None:
        self.samples: list[float] = []
        self.reset_called = False

    def feed(self, pcm: array.array[float]) -> FakeUpdate:
        self.samples = list(pcm)
        return FakeUpdate()

    def finalize(self) -> FakeUpdate:
        return FakeUpdate(revision=3, audio_committed_ms=320, buffered_ms=0)

    def text(self) -> SimpleNamespace:
        return SimpleNamespace(committed="Xin chào", tentative=" bạn", display="Xin chào bạn")

    def snapshot(self) -> SimpleNamespace:
        return SimpleNamespace(language="vi-VN")

    def reset(self) -> None:
        self.reset_called = True


class FakeBindingSession:
    def __init__(self, stream: FakeBindingStream) -> None:
        self.binding_stream = stream
        self.closed = False

    def stream(self, *, language: str) -> FakeBindingStream:
        assert language == "vi-VN"
        return self.binding_stream

    def close(self) -> None:
        self.closed = True


@pytest.mark.asyncio
async def test_native_asr_stream_converts_pcm_and_releases_lock(tmp_path: Path) -> None:
    service = AsrService(
        data_dir=tmp_path,
        executable=None,
        model_path=tmp_path / "model.gguf",
        language="vi-VN",
    )
    binding_stream = FakeBindingStream()
    session = FakeBindingSession(binding_stream)
    await service._lock.acquire()
    stream = AsrStream(service, session, binding_stream, "vi-VN")

    partial = await stream.feed(array.array("f", [0.25, -0.5]).tobytes())
    assert binding_stream.samples == pytest.approx([0.25, -0.5])
    assert partial["display"] == "Xin chào bạn"
    result = await stream.finalize()

    assert result["text"] == "Xin chào"
    assert result["language"] == "vi-VN"
    assert session.closed is True
    assert binding_stream.reset_called is True
    assert service._lock.locked() is False


@pytest.mark.asyncio
async def test_native_model_is_cached_until_service_shutdown(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = tmp_path / "source"
    (source / "build-shared" / "src").mkdir(parents=True)
    (source / "bindings" / "python" / "src" / "transcribe_cpp").mkdir(parents=True)
    (source / "build-shared" / "src" / "libtranscribe.dylib").write_bytes(b"native")
    (source / "bindings" / "python" / "src" / "transcribe_cpp" / "__init__.py").write_text("")
    model_path = tmp_path / "model.gguf"
    model_path.write_bytes(b"model")
    leases = GpuLeaseManager(capacity_bytes=100)
    binding_stream = FakeBindingStream()
    session = FakeBindingSession(binding_stream)

    class FakeModel:
        capabilities = SimpleNamespace(supports_streaming=True)

        def __init__(self) -> None:
            self.closed = False

        def session(self) -> FakeBindingSession:
            return session

        def close(self) -> None:
            self.closed = True

    model = FakeModel()
    service = AsrService(
        data_dir=tmp_path,
        executable=None,
        model_path=model_path,
        language="vi-VN",
        gpu_leases=leases,
        vram_reservation_bytes=30,
    )
    monkeypatch.setattr(
        service,
        "_load_binding_sync",
        lambda: SimpleNamespace(Model=lambda _path: model),
    )

    stream = await service.open_stream(language="vi-VN")
    assert leases.reserved_bytes == 30
    await stream.close()
    assert leases.reserved_bytes == 30
    await service.close()

    assert model.closed is True
    assert leases.reserved_bytes == 0


@pytest.mark.asyncio
async def test_asr_holds_and_releases_gpu_lease(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    leases = GpuLeaseManager(capacity_bytes=100)
    service = AsrService(
        data_dir=tmp_path,
        executable=None,
        model_path=tmp_path / "model.gguf",
        language="vi-VN",
        gpu_leases=leases,
        vram_reservation_bytes=30,
    )

    def fake_transcribe(content: bytes, filename: str, language: str) -> dict[str, str]:
        assert leases.reserved_bytes == 30
        assert content == b"audio"
        assert filename == "voice.webm"
        assert language == "vi-VN"
        return {"text": "Xin chào"}

    monkeypatch.setattr(service, "_transcribe_sync", fake_transcribe)
    result = await service.transcribe(b"audio", filename="voice.webm")

    assert result["text"] == "Xin chào"
    assert leases.reserved_bytes == 0
