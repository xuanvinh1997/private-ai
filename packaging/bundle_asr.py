"""Copy the compiled transcribe.cpp runtime into a built ``.app``.

Not a PyInstaller ``datas`` entry, for one reason: every ``.dylib`` here was linked with
``LC_RPATH`` pointing at an **absolute** path inside the build machine's ``.local-data``.
Copying the files alone produces a bundle that loads on the machine that built it and
nowhere else — ``@rpath/libggml.0.dylib`` resolves through a directory the recipient does
not have. So the copy is followed by rewriting each rpath to the ``@loader_path``-relative
equivalent, which is only correct if the tree keeps its shape. It does: the layout under
``build-shared`` is preserved exactly, so the relative hops are the ones the linker
already recorded.

This runs after PyInstaller and **before** codesign, because ``install_name_tool`` edits a
Mach-O header and invalidates any signature it already carried.

The Python bindings come along too. They are pure Python and load the library through
``ctypes``, honouring ``TRANSCRIBE_LIBRARY``, which is what ``AsrService`` sets.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

# Relative to the transcribe.cpp source tree: the directories holding shared libraries,
# and the bindings package. Anything outside these is build scaffolding.
LIBRARY_DIRS = (
    Path("build-shared") / "src",
    Path("build-shared") / "ggml" / "src",
    Path("build-shared") / "ggml" / "src" / "ggml-metal",
)
BINDINGS_DIR = Path("bindings") / "python" / "src" / "transcribe_cpp"

LIBRARY_SUFFIXES = (".dylib", ".so")


class BundleError(RuntimeError):
    """The runtime could not be embedded. The caller decides whether that is fatal."""


def _copy_libraries(source: Path, destination: Path) -> list[Path]:
    """Copy each library directory, keeping symlinks as symlinks.

    ``libtranscribe.dylib -> libtranscribe.0.2.dylib -> libtranscribe.0.2.2.dylib`` is the
    chain the resolver walks and the linker records. Dereferencing it would triple the
    payload and leave three files that no longer agree about which is canonical.
    """
    copied: list[Path] = []
    for relative in LIBRARY_DIRS:
        origin = source / relative
        if not origin.is_dir():
            continue
        target_dir = destination / relative
        target_dir.mkdir(parents=True, exist_ok=True)
        for item in sorted(origin.iterdir()):
            if item.suffix not in LIBRARY_SUFFIXES and ".dylib" not in item.name:
                continue
            target = target_dir / item.name
            if target.exists() or target.is_symlink():
                target.unlink()
            if item.is_symlink():
                os.symlink(os.readlink(item), target)
                continue
            shutil.copy2(item, target)
            copied.append(target)
    if not copied:
        raise BundleError(f"Không thấy thư viện transcribe.cpp nào trong {source}")
    return copied


def _rpaths(library: Path) -> list[str]:
    output = subprocess.run(  # noqa: S603
        ["/usr/bin/otool", "-l", str(library)],
        capture_output=True,
        text=True,
        check=False,
    ).stdout
    found: list[str] = []
    lines = output.splitlines()
    for index, line in enumerate(lines):
        if "LC_RPATH" not in line:
            continue
        for follow in lines[index : index + 4]:
            stripped = follow.strip()
            if stripped.startswith("path "):
                found.append(stripped.split(" (offset", 1)[0][len("path ") :].strip())
                break
    return found


def _relocate(library: Path, *, source_root: Path, dest_root: Path) -> list[tuple[str, str]]:
    """Turn every absolute rpath into the ``@loader_path`` hop that means the same thing.

    The hop is measured in the *original* tree — from where this library used to sit to
    where the rpath pointed — and then applied in the copied tree, which is valid only
    because the copy preserved the layout. Measuring it against the copied location
    instead would produce a path back out into the build machine's ``.local-data``, which
    is the bug this whole file exists to avoid.

    An rpath that is already ``@``-relative is portable and left alone. One whose target
    did not come along is deleted rather than rewritten: a dangling rpath sends dyld
    looking through a directory the recipient does not have.
    """
    relative = library.relative_to(dest_root)
    original_parent = source_root / relative.parent
    changes: list[tuple[str, str]] = []
    for original in _rpaths(library):
        if original.startswith("@"):
            continue
        hop = os.path.relpath(original, original_parent)
        landing = Path(os.path.normpath(library.parent / hop))
        portable = landing.is_dir() and landing.is_relative_to(dest_root)
        command = (
            ["-rpath", original, f"@loader_path/{hop}"] if portable else ["-delete_rpath", original]
        )
        subprocess.run(  # noqa: S603
            ["/usr/bin/install_name_tool", *command, str(library)],
            check=True,
            capture_output=True,
        )
        changes.append((original, f"@loader_path/{hop}" if portable else "(bỏ)"))
    return changes


def embed(source: Path, app: Path) -> Path:
    """Put ``source`` inside ``app`` at the layout ``AsrService`` already looks for."""
    source = source.expanduser().resolve()
    if not (source / "build-shared").is_dir():
        raise BundleError(
            f"Chưa dựng transcribe.cpp tại {source}. Chạy 'private-ai-asr setup' trên máy "
            "build trước, rồi build lại."
        )
    destination = app / "Contents" / "Frameworks" / "asr" / "source"
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)

    libraries = _copy_libraries(source, destination)
    for library in libraries:
        relocations = _relocate(library, source_root=source, dest_root=destination)
        for original, replacement in relocations:
            print(f"    {library.name}: {original} -> {replacement}")

    bindings_origin = source / BINDINGS_DIR
    if not (bindings_origin / "__init__.py").is_file():
        raise BundleError(f"Không thấy binding Python tại {bindings_origin}")
    bindings_target = destination / BINDINGS_DIR
    bindings_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(
        bindings_origin,
        bindings_target,
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
    )
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="Đường dẫn tới .app đã build")
    parser.add_argument(
        "--source",
        type=Path,
        default=None,
        help="Cây transcribe.cpp; mặc định lấy từ asr_dir của Settings",
    )
    arguments = parser.parse_args()

    source = arguments.source
    if source is None:
        from private_ai.config import get_settings

        source = get_settings().asr_dir / "source"
    try:
        destination = embed(source, arguments.app)
    except BundleError as error:
        print(f"Bỏ qua ASR: {error}", file=sys.stderr)
        return 1
    print(f"    nhúng {destination}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
