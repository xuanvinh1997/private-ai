"""Where generated files land, and under what name.

An artifact is something the agent *made* — a chart page, a diagram, a Word file, a
deck — as opposed to a document the user ingested. The two never share a directory:
anything under ``documents/`` is user input that gets parsed, chunked and indexed, and
letting a model write there would let it feed itself its own output as a source.

Nothing here overwrites. Every write gets a timestamped name, so a second run of the
same request produces a second file rather than silently replacing the first, and no
tool in this package can delete or truncate one.
"""

from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

__all__ = ["Artifact", "ArtifactError", "ArtifactStore", "slugify"]

# A workspace id is a UUID in practice. Anything that is not is refused rather than
# sanitised: a folder name built from an unexpected string is how ``..`` gets in.
_WORKSPACE_RE = re.compile(r"\A[A-Za-z0-9][A-Za-z0-9._-]{0,63}\Z")

SHARED_FOLDER = "chung"

MAX_SLUG_LENGTH = 60


class ArtifactError(RuntimeError):
    """A file could not be produced. Carries text meant for the model to read."""


def slugify(title: str, *, fallback: str = "artifact") -> str:
    """A filename stem from a Vietnamese title: tones folded, spaces to dashes."""
    folded = unicodedata.normalize("NFKD", title.casefold().replace("đ", "d"))
    ascii_text = "".join(char for char in folded if not unicodedata.combining(char))
    slug = re.sub(r"[^a-z0-9]+", "-", ascii_text).strip("-")
    return slug[:MAX_SLUG_LENGTH].strip("-") or fallback


@dataclass(frozen=True, slots=True)
class Artifact:
    """One file on disk, as the tools report it."""

    path: Path
    kind: str
    title: str
    byte_size: int
    created_at: str

    def public(self) -> dict[str, object]:
        return {
            "path": str(self.path),
            "filename": self.path.name,
            "kind": self.kind,
            "title": self.title,
            "byte_size": self.byte_size,
            "created_at": self.created_at,
        }


# Suffix -> what to call it when listing a folder we did not write in this process.
_KINDS = {
    ".html": "chart",
    ".mmd": "diagram-source",
    ".docx": "document",
    ".pptx": "slides",
}


class ArtifactStore:
    """The ``artifacts/`` tree, one subfolder per workspace."""

    def __init__(self, root: Path) -> None:
        self._root = Path(root)

    @property
    def root(self) -> Path:
        return self._root

    def folder(self, workspace_id: str = "") -> Path:
        """The directory a workspace's files go in, created on demand.

        An empty id is legitimate — a standalone server has no workspace — and shares one
        folder. An id that is not a plain token is a bug or an attack, and raises.
        """
        name = workspace_id.strip()
        if not name:
            name = SHARED_FOLDER
        elif not _WORKSPACE_RE.match(name):
            raise ArtifactError(f"Workspace id không hợp lệ: {workspace_id!r}")
        target = self._root / name
        try:
            target.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            raise ArtifactError(f"Không tạo được thư mục {target}: {exc}") from exc
        return target

    def _reserve(self, workspace_id: str, title: str, suffix: str) -> Path:
        folder = self.folder(workspace_id)
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        stem = f"{slugify(title)}-{stamp}"
        candidate = folder / f"{stem}{suffix}"
        # Two calls inside the same second would collide, so count up rather than clobber.
        counter = 2
        while candidate.exists():
            candidate = folder / f"{stem}-{counter}{suffix}"
            counter += 1
        return candidate

    def write_text(self, workspace_id: str, title: str, suffix: str, text: str) -> Path:
        target = self._reserve(workspace_id, title, suffix)
        try:
            target.write_text(text, encoding="utf-8")
        except OSError as exc:
            raise ArtifactError(f"Không ghi được {target}: {exc}") from exc
        return target

    def write_bytes(self, workspace_id: str, title: str, suffix: str, payload: bytes) -> Path:
        target = self._reserve(workspace_id, title, suffix)
        try:
            target.write_bytes(payload)
        except OSError as exc:
            raise ArtifactError(f"Không ghi được {target}: {exc}") from exc
        return target

    def describe(self, path: Path, kind: str, title: str) -> Artifact:
        stat = path.stat()
        return Artifact(
            path=path,
            kind=kind,
            title=title,
            byte_size=stat.st_size,
            created_at=datetime.fromtimestamp(stat.st_mtime, UTC).isoformat(),
        )

    def listing(self, workspace_id: str = "", limit: int = 50) -> list[Artifact]:
        """Newest first. A folder that was never written to is simply empty."""
        folder = self._root / (workspace_id.strip() or SHARED_FOLDER)
        if not folder.is_dir():
            return []
        found: list[Artifact] = []
        for item in folder.iterdir():
            if not item.is_file() or item.name.startswith("."):
                continue
            found.append(self.describe(item, _KINDS.get(item.suffix.lower(), "file"), item.stem))
        found.sort(key=lambda entry: entry.created_at, reverse=True)
        return found[: max(0, limit)]
