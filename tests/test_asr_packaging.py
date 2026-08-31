"""The speech stack's two packaging-shaped problems.

Neither shows up when the app is run from a checkout, which is why they both shipped. On
a developer's machine ``private-ai-asr setup`` has already put a compiled runtime and a
half-gigabyte model under ``.local-data/asr``, so ``health()`` is true and the microphone
lights up. A packaged build starts from nothing, has no console script to run setup with,
and no compiler to run it on — the button simply stays grey with nothing to click.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

from private_ai.asr import download as asr_download
from private_ai.asr.service import AsrService, bundled_asr_source

MODEL_NAME = "nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf"


def _service(tmp_path: Path) -> AsrService:
    return AsrService(
        data_dir=tmp_path / "asr",
        executable=None,
        model_path=tmp_path / "asr" / "models" / MODEL_NAME,
        language="vi-VN",
    )


def _fake_tree(root: Path) -> Path:
    """The subset of a transcribe.cpp build the resolvers actually look at."""
    library = root / "build-shared" / "src" / "libtranscribe.dylib"
    library.parent.mkdir(parents=True, exist_ok=True)
    library.write_bytes(b"\0")
    binding = root / "bindings" / "python" / "src" / "transcribe_cpp"
    binding.mkdir(parents=True, exist_ok=True)
    (binding / "__init__.py").write_text("", encoding="utf-8")
    return root


# --- finding the runtime --------------------------------------------------


def test_nothing_is_bundled_when_running_from_a_checkout() -> None:
    assert bundled_asr_source() is None


def test_a_frozen_build_finds_the_runtime_shipped_beside_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bundled = _fake_tree(tmp_path / "Frameworks" / "asr" / "source")
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "_MEIPASS", str(tmp_path / "Frameworks"), raising=False)

    service = _service(tmp_path / "data")
    assert bundled_asr_source() == bundled
    assert service.resolve_native_library() == (bundled / "build-shared/src/libtranscribe.dylib")
    assert service.resolve_binding_source() == bundled / "bindings" / "python" / "src"


def test_a_locally_built_runtime_wins_over_the_bundled_one(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """It was compiled on and for this machine; the bundled copy is the fallback."""
    _fake_tree(tmp_path / "Frameworks" / "asr" / "source")
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "_MEIPASS", str(tmp_path / "Frameworks"), raising=False)

    service = _service(tmp_path / "data")
    local = _fake_tree(tmp_path / "data" / "asr" / "source")
    assert service.resolve_native_library() == (local / "build-shared/src/libtranscribe.dylib")


async def test_a_bundled_runtime_without_weights_is_not_yet_usable(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The state a packaged app starts in, and the reason the models screen has a button:
    everything is present except the one file that cannot be shipped."""
    _fake_tree(tmp_path / "Frameworks" / "asr" / "source")
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "_MEIPASS", str(tmp_path / "Frameworks"), raising=False)

    service = _service(tmp_path / "data")
    status = service.status()
    assert status["native_library"] and status["binding_source"]
    assert status["model"] is None
    assert await service.health() is False


# --- fetching the weights -------------------------------------------------


class _Response:
    def __init__(self, payload: bytes, *, etag: str | None = None) -> None:
        self._payload = payload
        self.headers = {"content-length": str(len(payload))}
        if etag is not None:
            self.headers["etag"] = f'"{etag}"'
        self._offset = 0

    def read(self, size: int) -> bytes:
        chunk = self._payload[self._offset : self._offset + size]
        self._offset += len(chunk)
        return chunk

    def __enter__(self) -> _Response:
        return self

    def __exit__(self, *_: object) -> None:
        return None


def _serve(monkeypatch: pytest.MonkeyPatch, payload: bytes, *, etag: str | None = None) -> None:
    monkeypatch.setattr(
        asr_download.urllib.request, "urlopen", lambda _url: _Response(payload, etag=etag)
    )


def test_the_download_reports_progress_and_records_the_digest(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload = b"x" * (asr_download.CHUNK_BYTES * 2 + 5)
    digest = asr_download.hashlib.sha256(payload).hexdigest()
    _serve(monkeypatch, payload, etag=digest)
    target = tmp_path / "model.gguf"
    seen: list[tuple[int, int]] = []

    asr_download.download(
        "https://example/model", target, on_progress=lambda a, b: seen.append((a, b))
    )

    assert target.read_bytes() == payload
    assert asr_download.checksum_path(target).read_text(encoding="ascii").strip() == digest
    assert seen[0] == (0, len(payload))
    assert seen[-1] == (len(payload), len(payload))


def test_a_corrupted_download_leaves_nothing_behind(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A half-written GGUF loads far enough to fail somewhere much less obvious."""
    _serve(monkeypatch, b"not the model", etag="0" * 64)
    target = tmp_path / "model.gguf"

    with pytest.raises(RuntimeError, match="integrity"):
        asr_download.download("https://example/model", target)

    assert not target.exists()
    assert not target.with_suffix(".gguf.part").exists()


def test_cancelling_removes_the_partial_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _serve(monkeypatch, b"y" * (asr_download.CHUNK_BYTES * 3))
    target = tmp_path / "model.gguf"

    with pytest.raises(InterruptedError):
        asr_download.download("https://example/model", target, should_cancel=lambda: True)

    assert not target.exists()
    assert not target.with_suffix(".gguf.part").exists()


def test_an_already_complete_model_is_not_downloaded_again(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    target = tmp_path / "model.gguf"
    target.write_bytes(b"z" * (asr_download.MIN_MODEL_BYTES + 1))

    def refuse(_url: str) -> None:  # pragma: no cover - must never run
        raise AssertionError("đã tải lại một mô hình đã có")

    monkeypatch.setattr(asr_download.urllib.request, "urlopen", refuse)
    asr_download.download("https://example/model", target)
    assert asr_download.checksum_path(target).is_file()
