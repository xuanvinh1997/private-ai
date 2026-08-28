from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from uuid import uuid4

from private_ai_api.database import Database

DEFAULT_MAX_READ_BYTES = 1024 * 1024
DEFAULT_MAX_ENTRIES = 200
# Enough to spot a NUL byte in anything that is not really text.
BINARY_SNIFF_BYTES = 4096


class FileAccessError(ValueError):
    """The path cannot be served: it is missing, unreadable, or the wrong kind."""


class FileAccessDenied(FileAccessError):
    """The user has not allowed this path, and the client could not ask them."""


@dataclass(frozen=True, slots=True)
class FileGrant:
    id: str
    path: Path
    recursive: bool
    created_at: str

    def public(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "path": str(self.path),
            "recursive": self.recursive,
            "created_at": self.created_at,
        }


class FileAccessService:
    """Reads local files, but only under paths the user has actually allowed.

    Two things open a path: a root configured up front (``PRIVATE_AI_FILE_ROOTS``), and a
    grant the user gave at the moment a tool asked for it. Grants are stored so the same
    folder is only ever approved once.
    """

    def __init__(
        self,
        database: Database,
        *,
        roots: Sequence[Path] = (),
        protected: Sequence[Path] = (),
        max_read_bytes: int = DEFAULT_MAX_READ_BYTES,
        max_entries: int = DEFAULT_MAX_ENTRIES,
    ) -> None:
        self.database = database
        self.roots = tuple(dict.fromkeys(_normalize(item) for item in roots))
        self.protected = tuple(dict.fromkeys(_normalize(item) for item in protected))
        self.max_read_bytes = max_read_bytes
        self.max_entries = max_entries

    # -- permissions ---------------------------------------------------------

    def grants(self) -> list[FileGrant]:
        rows = self.database.fetch_all(
            "SELECT id, path, recursive, created_at FROM file_access_grants ORDER BY created_at"
        )
        return [
            FileGrant(
                id=str(row["id"]),
                path=Path(str(row["path"])),
                recursive=bool(row["recursive"]),
                created_at=str(row["created_at"]),
            )
            for row in rows
        ]

    def allowed_paths(self) -> list[Path]:
        return [*self.roots, *(grant.path for grant in self.grants())]

    def is_allowed(self, path: Path) -> bool:
        resolved = _normalize(path)
        if self.is_protected(resolved):
            return False
        return _within(resolved, self.allowed_paths())

    def is_protected(self, path: Path) -> bool:
        """The MCP token is the key to every other tool here, so it is never readable."""
        resolved = _normalize(path)
        return any(resolved == item for item in self.protected)

    def remember(self, path: Path) -> FileGrant:
        """Store the folder the user approved, so the next call does not ask again."""
        target = _normalize(path)
        folder = target if target.is_dir() else target.parent
        existing = self.database.fetch_one(
            "SELECT id, path, recursive, created_at FROM file_access_grants WHERE path = ?",
            (str(folder),),
        )
        if existing:
            return FileGrant(
                id=str(existing["id"]),
                path=Path(str(existing["path"])),
                recursive=bool(existing["recursive"]),
                created_at=str(existing["created_at"]),
            )
        grant = FileGrant(
            id=str(uuid4()),
            path=folder,
            recursive=True,
            created_at=datetime.now(UTC).isoformat(),
        )
        self.database.execute(
            "INSERT INTO file_access_grants(id, path, recursive, created_at) VALUES (?, ?, 1, ?)",
            (grant.id, str(grant.path), grant.created_at),
        )
        return grant

    def forget(self, grant_id: str) -> bool:
        if not self.database.fetch_one(
            "SELECT id FROM file_access_grants WHERE id = ?",
            (grant_id,),
        ):
            return False
        self.database.execute("DELETE FROM file_access_grants WHERE id = ?", (grant_id,))
        return True

    # -- reading -------------------------------------------------------------

    def resolve(self, raw: str) -> Path:
        """Resolve before every check, so ``..`` and symlinks cannot escape a root."""
        candidate = str(raw).strip()
        if not candidate:
            raise FileAccessError("Path cannot be empty")
        return _normalize(Path(candidate))

    def list_directory(self, path: Path, limit: int = 0) -> dict[str, Any]:
        if not path.exists():
            raise FileAccessError(f"No such path: {path}")
        if not path.is_dir():
            raise FileAccessError(f"Not a directory: {path}")
        count = max(1, min(limit or self.max_entries, self.max_entries))
        try:
            entries = sorted(path.iterdir(), key=lambda item: (not item.is_dir(), item.name))
        except OSError as exc:
            raise FileAccessError(f"Cannot list {path}: {exc}") from exc
        listed = [_entry(item) for item in entries[:count] if not self.is_protected(item)]
        return {
            "path": str(path),
            "entries": listed,
            "total_entries": len(entries),
            "truncated": len(entries) > count,
        }

    def read_file(self, path: Path, max_bytes: int = 0) -> dict[str, Any]:
        if not path.exists():
            raise FileAccessError(f"No such path: {path}")
        if not path.is_file():
            raise FileAccessError(f"Not a file: {path}")
        cap = max(1, min(max_bytes or self.max_read_bytes, self.max_read_bytes))
        try:
            size = path.stat().st_size
            with path.open("rb") as handle:
                payload = handle.read(cap)
        except OSError as exc:
            raise FileAccessError(f"Cannot read {path}: {exc}") from exc
        if b"\x00" in payload[:BINARY_SNIFF_BYTES]:
            raise FileAccessError(
                f"{path} looks like a binary file; upload it as a document instead"
            )
        return {
            "path": str(path),
            "content": payload.decode("utf-8", errors="replace"),
            "byte_size": size,
            "bytes_returned": len(payload),
            "truncated": size > len(payload),
        }


def _entry(item: Path) -> dict[str, Any]:
    try:
        stat = item.stat()
        size = stat.st_size
        modified = datetime.fromtimestamp(stat.st_mtime, UTC).isoformat()
    except OSError:
        size, modified = 0, None
    return {
        "name": item.name,
        "path": str(item),
        "type": "directory" if item.is_dir() else "file",
        "byte_size": size,
        "modified_at": modified,
    }


def _normalize(path: Path) -> Path:
    return Path(path).expanduser().resolve()


def _within(path: Path, roots: Iterable[Path]) -> bool:
    return any(path == root or path.is_relative_to(root) for root in roots)
