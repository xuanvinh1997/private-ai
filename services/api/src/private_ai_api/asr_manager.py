from __future__ import annotations

import argparse
import hashlib
import shutil
import subprocess
import urllib.request
from pathlib import Path

from private_ai_api.config import Settings
from private_ai_api.services.asr import AsrService

SOURCE_URL = "https://github.com/handy-computer/transcribe.cpp.git"
MODEL_URL = (
    "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/"
    "resolve/main/nemotron-3.5-asr-streaming-0.6b-Q4_K_M.gguf"
)


def checksum_path(target: Path) -> Path:
    return target.with_suffix(f"{target.suffix}.sha256")


def file_sha256(target: Path) -> str:
    digest = hashlib.sha256()
    with target.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def required_executable(name: str) -> str:
    found = shutil.which(name) or shutil.which(f"{name}.exe")
    if not found:
        raise RuntimeError(f"Required build tool is not installed: {name}")
    return found


def download(url: str, target: Path, *, force: bool = False) -> None:
    if not force and target.is_file() and target.stat().st_size > 100_000_000:
        digest = file_sha256(target)
        manifest = checksum_path(target)
        if manifest.is_file() and manifest.read_text(encoding="ascii").strip() != digest:
            raise RuntimeError("Existing ASR model failed SHA-256 integrity validation")
        manifest.write_text(f"{digest}\n", encoding="ascii")
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_suffix(f"{target.suffix}.part")
    digest = hashlib.sha256()
    with urllib.request.urlopen(url) as response, partial.open("wb") as destination:
        total = int(response.headers.get("content-length", "0"))
        etag = response.headers.get("etag", "").strip('"').casefold()
        expected = (
            etag
            if len(etag) == 64 and all(char in "0123456789abcdef" for char in etag)
            else ""
        )
        copied = 0
        while chunk := response.read(1024 * 1024):
            destination.write(chunk)
            digest.update(chunk)
            copied += len(chunk)
            if total:
                print(f"\rDownloading ASR model: {copied / total:.0%}", end="", flush=True)
    print()
    actual = digest.hexdigest()
    if expected and actual != expected:
        partial.unlink(missing_ok=True)
        raise RuntimeError("Downloaded ASR model failed SHA-256 integrity validation")
    partial.replace(target)
    checksum_path(target).write_text(f"{actual}\n", encoding="ascii")


def setup(settings: Settings) -> None:
    source = settings.asr_dir / "source"
    if not source.is_dir():
        source.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            [required_executable("git"), "clone", "--depth", "1", SOURCE_URL, str(source)],
            check=True,
        )
    build = source / "build"
    subprocess.run(
        [required_executable("cmake"), "-S", str(source), "-B", str(build)],
        check=True,
    )
    subprocess.run(
        [
            required_executable("cmake"),
            "--build",
            str(build),
            "--target",
            "transcribe-cli",
            "--config",
            "Release",
        ],
        check=True,
    )
    shared_build = source / "build-shared"
    subprocess.run(
        [
            required_executable("cmake"),
            "-S",
            str(source),
            "-B",
            str(shared_build),
            "-DTRANSCRIBE_BUILD_SHARED=ON",
            "-DTRANSCRIBE_BUILD_TESTS=OFF",
            "-DTRANSCRIBE_BUILD_EXAMPLES=OFF",
            "-DTRANSCRIBE_BUILD_TOOLS=OFF",
            "-DTRANSCRIBE_USE_OPENMP=OFF",
        ],
        check=True,
    )
    subprocess.run(
        [
            required_executable("cmake"),
            "--build",
            str(shared_build),
            "--target",
            "transcribe",
            "--config",
            "Release",
        ],
        check=True,
    )
    download(MODEL_URL, settings.default_asr_model_path)


def run() -> None:
    parser = argparse.ArgumentParser(description="Set up local transcribe.cpp ASR")
    parser.add_argument("action", choices=("setup", "status"), nargs="?", default="status")
    args = parser.parse_args()
    settings = Settings()
    if args.action == "setup":
        setup(settings)
    service = AsrService(
        data_dir=settings.asr_dir,
        executable=settings.asr_executable,
        model_path=settings.asr_model or settings.default_asr_model_path,
        language=settings.asr_language,
        ffmpeg_executable=settings.ffmpeg_executable,
        enabled=settings.asr_enabled,
    )
    state = service.status()
    print(f"ASR available: {state['available']}")
    print(f"Executable: {state['executable'] or 'missing'}")
    print(f"Native streaming: {state['streaming_available']}")
    print(f"Native library: {state['native_library'] or 'missing'}")
    print(f"Model: {state['model'] or 'missing'}")
    print(f"FFmpeg: {state['ffmpeg'] or 'missing'}")


if __name__ == "__main__":
    run()
