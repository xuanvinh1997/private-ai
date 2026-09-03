"""Kho siêu dữ liệu: SQLite giữ tài liệu, đoạn và chỉ mục từ khoá.

Vector **không** nằm ở đây — chúng ở Qdrant. Chia như vậy vì hai thứ có hình dạng truy
vấn khác hẳn nhau: đoạn văn cần lọc theo tài liệu, phân trang, và tìm toàn văn có dấu
tiếng Việt; vector cần láng giềng gần nhất trong không gian nhiều chiều. Nhét cả hai vào
một chỗ nghĩa là một trong hai chạy kém.

# Bốn quyết định

**1. FTS5 external content.** ``chunks_fts`` trỏ về ``chunks`` bằng ``content='chunks'``.
Mười nghìn đoạn ~1400 ký tự thì một bản sao tốn thêm mười bốn megabyte cho đúng dữ liệu
đã nằm ngay bên cạnh.

**2. ``remove_diacritics 2`` là bắt buộc, không phải tiện lợi.** Người Việt gõ tìm kiếm
không dấu suốt, và một chỉ mục phân biệt dấu thì "bao mat" không tìm ra "bảo mật". Mức 2
(chứ không phải 1) mới xử lý đúng các ký tự tổ hợp ngoài Latin-1.

**3. Đường dẫn là danh tính, ``mtime`` + kích thước là dấu vân tay.** Băm mọi tệp ở mọi
lần quét là đọc lại cả thư mục mỗi lần — đúng cái giá mà chỉ mục tăng dần sinh ra để khỏi
phải trả. Nó bỏ sót đúng một trường hợp: sửa tệp mà giữ nguyên cả độ dài lẫn ``mtime``.

**4. Xoá bằng lệnh tường minh, không dựa ``ON DELETE CASCADE``.** SQLite chỉ kích hoạt
trigger cho hàng xoá theo dây chuyền khi ``recursive_triggers`` bật. Trông vào cascade thì
hàng ``chunks`` biến mất còn chỉ mục FTS ở lại, và một hàng FTS mồ côi **vẫn trả kết
quả** — tìm ra đoạn của tài liệu đã xoá rồi không đọc nổi nó.
"""

from __future__ import annotations

import json
import re
import sqlite3
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from pai_rag_service.chunking import Chunk

__all__ = [
    "ChunkRow",
    "DocumentRow",
    "META_EMBEDDER",
    "META_EMBED_INPUT",
    "META_EXTRACT",
    "SCHEMA_VERSION",
    "Store",
]

SCHEMA_VERSION = 1

META_EMBEDDER = "embedder.id"
META_EMBEDDER_DIM = "embedder.dim"
META_EMBED_INPUT = "embed.input.version"
META_EXTRACT = "extract.version"
#: Số tệp lần quét gần nhất nhìn thấy, số tệp bỏ qua vì chạm trần, và lúc quét xong.
#: Ghi vào kho chứ không giữ trong bộ nhớ: giao diện phải nói được "quét lúc nào" ngay
#: khi vừa mở ứng dụng, trước lần quét đầu tiên của phiên.
META_SCAN_FILES = "scan.files"
META_SCAN_SKIPPED = "scan.skipped"
META_SCAN_AT = "scan.at"

SCHEMA = """
CREATE TABLE IF NOT EXISTS documents (
  id        TEXT PRIMARY KEY,
  path      TEXT NOT NULL UNIQUE,
  title     TEXT NOT NULL,
  format    TEXT NOT NULL,
  bytes     INTEGER NOT NULL,
  mtime     INTEGER NOT NULL,
  pages     INTEGER NOT NULL DEFAULT 0,
  ocr_pages TEXT NOT NULL DEFAULT '[]',
  added_at  INTEGER NOT NULL,
  error     TEXT
);

CREATE TABLE IF NOT EXISTS chunks (
  id          INTEGER PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  ordinal     INTEGER NOT NULL,
  section     TEXT NOT NULL DEFAULT '',
  page        INTEGER NOT NULL DEFAULT 0,
  body        TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS chunks_by_document ON chunks (document_id, ordinal);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  body, section, content = 'chunks', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts (rowid, body, section) VALUES (new.id, new.body, new.section);
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, section)
  VALUES ('delete', old.id, old.body, old.section);
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, section)
  VALUES ('delete', old.id, old.body, old.section);
  INSERT INTO chunks_fts (rowid, body, section) VALUES (new.id, new.body, new.section);
END;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- Tệp đã thử đọc và không đọc được, kèm dấu vân tay lúc thử.
--
-- Không có bảng này thì mỗi lần quét lại đi rút chữ lại đúng những tệp đã hỏng — một PDF
-- cụt ở mọi lần mở dự án — và bất biến "quét lại một thư mục không đổi thì không rút chữ
-- lại tệp nào" chỉ còn đúng với thư mục toàn tệp lành.
-- Tệp còn nằm trong thư mục dự án nhưng người dùng đã bỏ khỏi thư viện.
--
-- Bảng này tồn tại vì `remove` không xoá tệp của người dùng: không có nó thì lần quét
-- ngay sau đó nạp lại đúng cái tài liệu họ vừa bỏ đi, và một nút bấm không có tác dụng
-- là một nút bấm dạy người dùng rằng phần mềm không nghe lời họ.
CREATE TABLE IF NOT EXISTS excluded (
  path TEXT PRIMARY KEY,
  at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS failures (
  path   TEXT PRIMARY KEY,
  mtime  INTEGER NOT NULL,
  size   INTEGER NOT NULL,
  reason TEXT NOT NULL
);
"""


@dataclass(slots=True)
class DocumentRow:
    id: str
    path: str
    title: str
    format: str
    bytes: int
    mtime: int
    pages: int
    ocr_pages: list[int]
    added_at: int
    error: str | None
    chunks: int


@dataclass(slots=True)
class ChunkRow:
    id: int
    document_id: str
    title: str
    path: str
    ordinal: int
    section: str
    page: int
    body: str


def _fts_expressions(query: str) -> tuple[str, str] | None:
    """``(biểu thức AND, biểu thức OR)`` từ một câu hỏi của người dùng.

    Chuỗi người dùng **không bao giờ** được ghép thẳng vào cú pháp ``MATCH``: ``"``,
    ``*``, ``:``, ``^``, ``NOT``, ``NEAR`` đều có nghĩa ở đó, nên một câu hỏi bình thường
    có thể thành lỗi cú pháp và một câu hỏi cố ý có thể thành một truy vấn khác hẳn. Cắt
    thành token rồi bọc nháy kép biến mọi thứ thành chữ nghĩa thuần tuý.
    """
    tokens = [f'"{token}"' for token in re.findall(r"[^\W_]+", query, re.UNICODE)]
    if not tokens:
        return None
    return " AND ".join(tokens), " OR ".join(tokens)


class Store:
    """Một tệp SQLite cho mỗi dự án tài liệu."""

    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        # `isolation_level=None` = tự commit mỗi câu lệnh, và transaction do ta mở
        # bằng tay. Chế độ ngầm của `sqlite3` mở transaction trước mỗi câu ghi rồi
        # để nó treo tới lần `commit()` kế tiếp — nên một câu ghi lẻ ở đâu đó khiến
        # `BEGIN` của phép ghi kế tiếp hỏng với "transaction within a transaction".
        self.conn = sqlite3.connect(str(path), check_same_thread=False, isolation_level=None)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute("PRAGMA journal_mode = WAL")
        self.conn.execute("PRAGMA foreign_keys = ON")
        self.conn.execute("PRAGMA synchronous = NORMAL")
        self.conn.executescript(SCHEMA)
        self.conn.commit()

    def close(self) -> None:
        # Gộp WAL lúc đóng: không có bước này thì thư mục ở lại với một tệp `-wal` mà lần
        # mở sau phải phát lại.
        try:
            self.conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        finally:
            self.conn.close()

    def __enter__(self) -> Store:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- meta -------------------------------------------------------------------------

    def meta(self, key: str) -> str | None:
        row = self.conn.execute("SELECT value FROM meta WHERE key = ?", (key,)).fetchone()
        return row["value"] if row else None

    def set_meta(self, key: str, value: str) -> None:
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )

    def identity(self) -> dict[str, str | None]:
        return {
            "embedder": self.meta(META_EMBEDDER),
            "dim": self.meta(META_EMBEDDER_DIM),
            "embed_input": self.meta(META_EMBED_INPUT),
            "extract": self.meta(META_EXTRACT),
        }

    def set_identity(
        self, *, embedder: str, dim: int | None, embed_input: int, extract: int
    ) -> None:
        self.set_meta(META_EMBEDDER, embedder)
        if dim is not None:
            self.set_meta(META_EMBEDDER_DIM, str(dim))
        self.set_meta(META_EMBED_INPUT, str(embed_input))
        self.set_meta(META_EXTRACT, str(extract))

    # -- dấu vân tay ------------------------------------------------------------------

    def known_files(self) -> dict[str, tuple[int, int]]:
        """``đường dẫn -> (mtime, kích thước)`` của mọi tệp đã nạp hoặc đã thử và hỏng.

        Gộp cả hai bảng vào một phép tra: lần quét sau phải bỏ qua **cả hai** nhóm, và
        hỏi hai lần là hai chỗ để quên một lần.
        """
        out: dict[str, tuple[int, int]] = {}
        for row in self.conn.execute("SELECT path, mtime, bytes FROM documents"):
            out[row["path"]] = (row["mtime"], row["bytes"])
        for row in self.conn.execute("SELECT path, mtime, size FROM failures"):
            out.setdefault(row["path"], (row["mtime"], row["size"]))
        return out

    def put_failure(self, path: str, mtime: int, size: int, reason: str) -> None:
        self.conn.execute(
            "INSERT INTO failures (path, mtime, size, reason) VALUES (?, ?, ?, ?) "
            "ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime, "
            "size = excluded.size, reason = excluded.reason",
            (path, mtime, size, reason),
        )

    def clear_failure(self, path: str) -> None:
        self.conn.execute("DELETE FROM failures WHERE path = ?", (path,))

    def failures(self) -> list[tuple[str, str]]:
        rows = self.conn.execute("SELECT path, reason FROM failures ORDER BY path")
        return [(row["path"], row["reason"]) for row in rows]

    def forget_fingerprints(self) -> int:
        """Quên mọi dấu vân tay, để lần quét tới đọc lại cả thư mục.

        Gọi khi bộ rút chữ hoặc cách dựng văn bản nhúng đã đổi. Không có bước này thì một
        bản vá ở tầng dưới chỉ tới được với thư viện mới.
        """
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            changed = cur.execute("UPDATE documents SET mtime = 0").rowcount
            cur.execute("DELETE FROM failures")
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        return changed

    # -- tài liệu ---------------------------------------------------------------------

    def put_document(
        self,
        *,
        doc_id: str,
        path: str,
        title: str,
        fmt: str,
        size: int,
        mtime: int,
        pages: int,
        ocr_pages: list[int],
        chunks: list[Chunk],
    ) -> list[int]:
        """Ghi một tài liệu và mọi đoạn của nó. Trả về mã đoạn theo đúng thứ tự.

        Thay thế toàn bộ khi tài liệu đã có: nạp lại một tệp đã sửa phải cho ra đúng
        trạng thái như nạp nó lần đầu, không phải trộn đoạn cũ với đoạn mới.
        """
        now = int(time.time() * 1000)
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            self._forget_chunks(cur, doc_id)
            cur.execute(
                "INSERT INTO documents (id, path, title, format, bytes, mtime, pages, "
                "ocr_pages, added_at, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL) "
                "ON CONFLICT(id) DO UPDATE SET path = excluded.path, title = excluded.title, "
                "format = excluded.format, bytes = excluded.bytes, mtime = excluded.mtime, "
                "pages = excluded.pages, ocr_pages = excluded.ocr_pages, error = NULL",
                (doc_id, path, title, fmt, size, mtime, pages, json.dumps(ocr_pages), now),
            )
            ids: list[int] = []
            for chunk in chunks:
                cur.execute(
                    "INSERT INTO chunks (document_id, ordinal, section, page, body) "
                    "VALUES (?, ?, ?, ?, ?)",
                    (doc_id, chunk.ordinal, chunk.section, chunk.page, chunk.text),
                )
                ids.append(int(cur.lastrowid))
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        self.conn.execute("DELETE FROM failures WHERE path = ?", (path,))
        return ids

    @staticmethod
    def _forget_chunks(cur: sqlite3.Cursor, doc_id: str) -> None:
        """Xoá đoạn của một tài liệu. Tường minh — xem quyết định 4 ở đầu tệp."""
        cur.execute("DELETE FROM chunks WHERE document_id = ?", (doc_id,))

    def remove_document(self, doc_id: str) -> list[int]:
        """Xoá một tài liệu. Trả về mã đoạn đã xoá, để người gọi dọn Qdrant theo."""
        rows = self.conn.execute("SELECT id FROM chunks WHERE document_id = ?", (doc_id,))
        ids = [int(row["id"]) for row in rows]
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            self._forget_chunks(cur, doc_id)
            cur.execute("DELETE FROM documents WHERE id = ?", (doc_id,))
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        return ids

    def documents(self) -> list[DocumentRow]:
        rows = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d ORDER BY d.added_at DESC"
        )
        return [self._document(row) for row in rows]

    def document(self, doc_id: str) -> DocumentRow | None:
        row = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d WHERE d.id = ?",
            (doc_id,),
        ).fetchone()
        return self._document(row) if row else None

    def document_by_path(self, path: str) -> DocumentRow | None:
        row = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d WHERE d.path = ?",
            (path,),
        ).fetchone()
        return self._document(row) if row else None

    @staticmethod
    def _document(row: sqlite3.Row) -> DocumentRow:
        return DocumentRow(
            id=row["id"],
            path=row["path"],
            title=row["title"],
            format=row["format"],
            bytes=row["bytes"],
            mtime=row["mtime"],
            pages=row["pages"],
            ocr_pages=json.loads(row["ocr_pages"] or "[]"),
            added_at=row["added_at"],
            error=row["error"],
            chunks=row["n"],
        )

    # -- đoạn -------------------------------------------------------------------------

    _CHUNK_SELECT = (
        "SELECT c.id, c.document_id, c.ordinal, c.section, c.page, c.body, "
        "d.title, d.path FROM chunks c JOIN documents d ON d.id = c.document_id "
    )

    def chunks_by_id(self, ids: list[int]) -> list[ChunkRow]:
        if not ids:
            return []
        marks = ",".join("?" * len(ids))
        rows = self.conn.execute(f"{self._CHUNK_SELECT} WHERE c.id IN ({marks})", ids)
        return [self._chunk(row) for row in rows]

    def chunks_of(self, doc_id: str, offset: int = 0, limit: int = 50) -> list[ChunkRow]:
        rows = self.conn.execute(
            f"{self._CHUNK_SELECT} WHERE c.document_id = ? ORDER BY c.ordinal LIMIT ? OFFSET ?",
            (doc_id, limit, offset),
        )
        return [self._chunk(row) for row in rows]

    @staticmethod
    def _chunk(row: sqlite3.Row) -> ChunkRow:
        return ChunkRow(
            id=row["id"],
            document_id=row["document_id"],
            title=row["title"],
            path=row["path"],
            ordinal=row["ordinal"],
            section=row["section"],
            page=row["page"],
            body=row["body"],
        )

    def counts(self) -> tuple[int, int]:
        docs = self.conn.execute("SELECT COUNT(*) AS n FROM documents").fetchone()["n"]
        chunks = self.conn.execute("SELECT COUNT(*) AS n FROM chunks").fetchone()["n"]
        return int(docs), int(chunks)

    # -- tìm theo từ khoá -------------------------------------------------------------

    def search_keyword(self, query: str, limit: int) -> list[int]:
        """Mã đoạn theo thứ tự BM25, tốt nhất trước.

        Cân ``section`` gấp đôi ``body``: một câu hỏi khớp đúng vào tiêu đề mục gần như
        luôn là câu hỏi về chính mục đó.
        """
        built = _fts_expressions(query)
        if built is None:
            return []
        strict, loose = built
        sql = (
            "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ? "
            "ORDER BY bm25(chunks_fts, 1.0, 2.0) LIMIT ?"
        )
        hits = [int(row[0]) for row in self.conn.execute(sql, (strict, limit))]
        if not hits:
            # Mọi từ cùng có mặt là phép lọc đúng khi nó trả về cái gì đó. Khi nó trả về
            # rỗng — người dùng gõ cả một câu hỏi chứ không phải một cụm từ khoá — thì
            # "có từ nào cũng được" còn hơn là không có gì.
            hits = [int(row[0]) for row in self.conn.execute(sql, (loose, limit))]
        return hits

    # -- loại trừ ---------------------------------------------------------------------

    def exclude(self, path: str, at: int) -> None:
        """Đánh dấu một tệp là đã bị người dùng bỏ khỏi thư viện."""
        self.conn.execute(
            "INSERT INTO excluded (path, at) VALUES (?, ?) "
            "ON CONFLICT(path) DO UPDATE SET at = excluded.at",
            (path, at),
        )

    def allow(self, path: str) -> None:
        """Bỏ dấu loại trừ — người dùng nạp lại tệp này một cách tường minh."""
        self.conn.execute("DELETE FROM excluded WHERE path = ?", (path,))

    def excluded(self) -> set[str]:
        return {row["path"] for row in self.conn.execute("SELECT path FROM excluded")}

    def clear_excluded(self) -> int:
        """Cho phép lại mọi tệp. Dùng khi người dùng bấm xử lý lại cả thư viện."""
        cur = self.conn.execute("DELETE FROM excluded")
        return cur.rowcount

    def integrity(self) -> None:
        """Ném nếu chỉ mục FTS lệch khỏi bảng nội dung. Dùng trong ``pai-rag doctor``."""
        self.conn.execute("INSERT INTO chunks_fts (chunks_fts) VALUES ('integrity-check')")

    def stats(self) -> dict[str, Any]:
        docs, chunks = self.counts()
        return {
            "documents": docs,
            "chunks": chunks,
            "failures": len(self.failures()),
            **self.identity(),
        }
