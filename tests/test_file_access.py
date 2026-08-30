"""Local file reads, gated on what the user has actually allowed.

Ported from ``services/api/tests/test_system_and_files.py``, minus the HTTP layer.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from private_ai.core.database import Database
from private_ai.core.file_access import FileAccessError, FileAccessService


@pytest.fixture
def tree(tmp_path: Path) -> Path:
    root = tmp_path / "allowed"
    (root / "nested").mkdir(parents=True)
    (root / "note.txt").write_text("xin chào", encoding="utf-8")
    (root / "nested" / "deep.md").write_text("# sâu", encoding="utf-8")
    (tmp_path / "elsewhere").mkdir()
    (tmp_path / "elsewhere" / "secret.txt").write_text("riêng tư", encoding="utf-8")
    return root


def test_a_configured_root_is_readable_and_everything_else_is_not(
    database: Database,
    tree: Path,
    tmp_path: Path,
) -> None:
    service = FileAccessService(database, roots=[tree])
    assert service.is_allowed(tree / "note.txt")
    assert service.is_allowed(tree / "nested" / "deep.md")
    assert not service.is_allowed(tmp_path / "elsewhere" / "secret.txt")


def test_dot_dot_cannot_escape_a_root(database: Database, tree: Path) -> None:
    """The path is resolved before every check, so traversal never gets a decision."""
    service = FileAccessService(database, roots=[tree])
    escaped = service.resolve(str(tree / ".." / "elsewhere" / "secret.txt"))
    assert not service.is_allowed(escaped)


def test_a_symlink_out_of_a_root_is_not_allowed(database: Database, tree: Path) -> None:
    target = tree.parent / "elsewhere" / "secret.txt"
    link = tree / "shortcut.txt"
    link.symlink_to(target)
    service = FileAccessService(database, roots=[tree])
    assert not service.is_allowed(service.resolve(str(link)))


def test_the_mcp_token_is_never_readable(database: Database, tmp_path: Path) -> None:
    token = tmp_path / "mcp-token"
    token.write_text("bí mật", encoding="utf-8")
    service = FileAccessService(database, roots=[tmp_path], protected=[token])

    assert service.is_protected(token)
    # Being inside an allowed root does not help: protection is checked first.
    assert not service.is_allowed(token)

    listing = service.list_directory(tmp_path)
    assert "mcp-token" not in {entry["name"] for entry in listing["entries"]}


def test_a_grant_is_remembered_once_and_reused(database: Database, tree: Path) -> None:
    service = FileAccessService(database, roots=())
    first = service.remember(tree / "note.txt")
    second = service.remember(tree / "nested")

    # A file grant is stored as its folder, so the next file in it needs no question.
    assert first.path == tree
    assert service.is_allowed(tree / "note.txt")
    assert service.remember(tree / "note.txt").id == first.id
    assert {grant.id for grant in service.grants()} == {first.id, second.id}

    assert service.forget(first.id) is True
    assert service.forget(first.id) is False
    assert not service.is_allowed(tree / "note.txt")


def test_reading_a_file_reports_truncation_rather_than_lying(
    database: Database,
    tmp_path: Path,
) -> None:
    target = tmp_path / "long.txt"
    target.write_text("a" * 500, encoding="utf-8")
    service = FileAccessService(database, roots=[tmp_path], max_read_bytes=100)

    result = service.read_file(target)
    assert result["bytes_returned"] == 100
    assert result["byte_size"] == 500
    assert result["truncated"] is True

    whole = service.read_file(tmp_path / "long.txt", max_bytes=1000)
    # The service cap wins over a larger request.
    assert whole["bytes_returned"] == 100


def test_a_binary_file_is_refused_instead_of_returning_mojibake(
    database: Database,
    tmp_path: Path,
) -> None:
    target = tmp_path / "image.bin"
    target.write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d")
    service = FileAccessService(database, roots=[tmp_path])
    with pytest.raises(FileAccessError, match="binary"):
        service.read_file(target)


def test_missing_and_wrong_kind_paths_raise(database: Database, tree: Path) -> None:
    service = FileAccessService(database, roots=[tree])
    with pytest.raises(FileAccessError):
        service.read_file(tree / "absent.txt")
    with pytest.raises(FileAccessError, match="Not a file"):
        service.read_file(tree / "nested")
    with pytest.raises(FileAccessError, match="Not a directory"):
        service.list_directory(tree / "note.txt")
    with pytest.raises(FileAccessError, match="empty"):
        service.resolve("   ")


def test_directory_listings_are_capped_and_say_so(database: Database, tmp_path: Path) -> None:
    folder = tmp_path / "many"
    folder.mkdir()
    for index in range(10):
        (folder / f"file-{index}.txt").write_text("x", encoding="utf-8")
    service = FileAccessService(database, roots=[folder], max_entries=4)

    listing = service.list_directory(folder)
    assert len(listing["entries"]) == 4
    assert listing["total_entries"] == 10
    assert listing["truncated"] is True


def test_directories_sort_before_files(database: Database, tree: Path) -> None:
    service = FileAccessService(database, roots=[tree])
    entries = service.list_directory(tree)["entries"]
    assert [entry["type"] for entry in entries] == ["directory", "file"]
