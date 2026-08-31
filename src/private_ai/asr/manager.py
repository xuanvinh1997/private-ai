"""``private-ai-asr`` — build transcribe.cpp locally and fetch the ASR model.

The binary is not shipped: it is compiled on the machine that will run it so the build
picks up that machine's accelerators. Both a CLI and a shared library come out of the
same checkout, because the batch path shells out and the streaming path links in.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess

from private_ai.asr.download import MODEL_URL, checksum_path, download, file_sha256
from private_ai.asr.service import AsrService
from private_ai.config import Settings, get_settings

SOURCE_URL = "https://github.com/handy-computer/transcribe.cpp.git"

# Re-exported so ``private_ai.asr.manager`` stays the one name for the setup surface even
# though the download itself is now shared with the desktop app.
__all__ = ["MODEL_URL", "checksum_path", "download", "file_sha256", "run", "setup"]


def required_executable(name: str) -> str:
    found = shutil.which(name) or shutil.which(f"{name}.exe")
    if not found:
        raise RuntimeError(f"Required build tool is not installed: {name}")
    return found


def _print_progress(copied: int, total: int) -> None:
    if total:
        print(f"\rDownloading ASR model: {copied / total:.0%}", end="", flush=True)


def setup(settings: Settings) -> None:
    source = settings.asr_dir / "source"
    if not source.is_dir():
        source.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(  # noqa: S603
            [required_executable("git"), "clone", "--depth", "1", SOURCE_URL, str(source)],
            check=True,
        )
    build = source / "build"
    subprocess.run(  # noqa: S603
        [required_executable("cmake"), "-S", str(source), "-B", str(build)],
        check=True,
    )
    subprocess.run(  # noqa: S603
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
    subprocess.run(  # noqa: S603
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
    subprocess.run(  # noqa: S603
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
    download(MODEL_URL, settings.default_asr_model_path, on_progress=_print_progress)
    print()


def run() -> None:
    parser = argparse.ArgumentParser(description="Set up local transcribe.cpp ASR")
    parser.add_argument("action", choices=("setup", "status"), nargs="?", default="status")
    args = parser.parse_args()
    settings = get_settings()
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
