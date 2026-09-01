//! Kho: SQLite, FTS5, và một bảng vector.
//!
//! Một tệp cơ sở dữ liệu cho **mỗi dự án tài liệu**, nằm cạnh thư mục chứa bản sao tệp.
//! Không dùng chung một kho cho mọi dự án, vì triệu chứng của việc dùng chung là
//! `docs.search` trả về một đoạn từ thư viện của dự án khác — một câu trả lời trông y hệt
//! một câu trả lời sai bình thường, nên không ai lần ra nguyên nhân.
//!
//! # Bốn quyết định
//!
//! **1. FTS5 external content.** Bảng `chunks_fts` không tự giữ nội dung; nó trỏ về
//! `chunks` bằng `content='chunks'` và được đồng bộ bằng ba trigger. Đây là chỗ **khác**
//! `pai-index`, và khác có lý do: ở đó mỗi hàng FTS là một cái tên vài chục byte nên lặp
//! lại không đáng kể, còn ở đây mỗi hàng là một đoạn ~1000 ký tự, và một thư viện mười
//! nghìn đoạn thì bản sao tốn thêm mười megabyte cho đúng cái dữ liệu đã nằm ngay bên
//! cạnh.
//!
//! Cái giá phải trả là `trusted_schema` **không** tắt được: trigger ghi vào một virtual
//! table là đúng thứ `trusted_schema = OFF` chặn, và tắt nó thì mọi lần chèn đoạn đều
//! hỏng. Đổi lại, ta kiểm `application_id` trước khi ghi và tệp này nằm trong thư mục dữ
//! liệu do chính ứng dụng tạo — nó không phải một tệp người dùng mở từ ngoài vào.
//!
//! **2. `sha256` là danh tính của nội dung.** Người dùng kéo cùng một tệp vào hai lần —
//! từ Downloads rồi từ Desktop — và hai hàng tài liệu giống hệt nhau trong danh sách là
//! một lỗi họ nhìn thấy ngay. Băm nội dung thay vì so đường dẫn, vì đường dẫn đổi còn nội
//! dung thì không.
//!
//! **3. Xoá bằng lệnh tường minh, không dựa vào `ON DELETE CASCADE`.** SQLite chỉ kích
//! hoạt trigger cho những hàng bị xoá theo dây chuyền khi `recursive_triggers` bật. Xoá
//! một tài liệu mà chỉ trông vào cascade thì hàng `chunks` biến mất còn chỉ mục FTS ở
//! lại, và một hàng FTS mồ côi **vẫn trả về kết quả** — tìm ra một đoạn của tài liệu đã
//! xoá rồi không đọc nổi nó. Vì thế [`Store::remove_document`] xoá vector, rồi đoạn, rồi
//! tài liệu, từng lệnh một.
//!
//! **4. Lệch schema thì dựng lại — nhưng chỉ khi còn dựng lại được.** Xem
//! [`ensure_schema`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::chunk::Chunk;
use crate::error::RagError;
use crate::extract::Format;

type Result<T> = std::result::Result<T, RagError>;

/// `'PRAG'`. Mở nhầm một tệp SQLite khác thì thấy ngay, trước khi ghi vào.
const APPLICATION_ID: i32 = 0x50524147;
/// Bản 2 thêm bảng `meta`, và cùng với nó là việc nhớ **bộ nhúng nào đã sinh ra những
/// vector đang nằm trong kho** — xem [`Store::forget_vectors`].
const SCHEMA_VERSION: i32 = 2;

const SCHEMA: &str = r#"
CREATE TABLE documents (
  id        TEXT    PRIMARY KEY,
  path      TEXT    NOT NULL,
  origin    TEXT    NOT NULL,
  title     TEXT    NOT NULL,
  format    TEXT    NOT NULL,
  sha256    TEXT    NOT NULL UNIQUE,
  bytes     INTEGER NOT NULL,
  added_at  INTEGER NOT NULL,
  error     TEXT
) STRICT;

CREATE TABLE chunks (
  id          INTEGER PRIMARY KEY,
  document_id TEXT    NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  ord         INTEGER NOT NULL,
  heading     TEXT,
  body        TEXT    NOT NULL,
  start_byte  INTEGER NOT NULL,
  end_byte    INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX chunks_by_document ON chunks (document_id, ord);

-- `remove_diacritics 2` là bắt buộc chứ không phải tiện lợi: người Việt gõ tìm kiếm
-- không dấu suốt, và một chỉ mục phân biệt dấu thì "bao mat" không tìm ra "bảo mật".
-- Mức 2 (chứ không phải 1) mới xử lý đúng các ký tự tổ hợp ngoài Latin-1.
CREATE VIRTUAL TABLE chunks_fts USING fts5(
  body, heading, content = 'chunks', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER chunks_after_insert AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts (rowid, body, heading) VALUES (new.id, new.body, new.heading);
END;

CREATE TRIGGER chunks_after_delete AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, heading)
  VALUES ('delete', old.id, old.body, old.heading);
END;

CREATE TRIGGER chunks_after_update AFTER UPDATE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, heading)
  VALUES ('delete', old.id, old.body, old.heading);
  INSERT INTO chunks_fts (rowid, body, heading) VALUES (new.id, new.body, new.heading);
END;

CREATE TABLE vectors (
  chunk_id INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
  dim      INTEGER NOT NULL,
  data     BLOB    NOT NULL
) STRICT;

-- Một bảng khoá–giá trị thay vì vài cột trên một bảng một hàng: những thứ ghi ở đây đều
-- là chuyện *của kho*, không phải của tài liệu nào, và chúng sẽ còn thêm. Một bảng một
-- hàng thì mỗi lần thêm một thứ là một lần đổi schema.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
"#;

/// Danh tính bộ nhúng đã sinh ra những vector đang nằm trong kho.
pub const META_EMBEDDER_ID: &str = "embedder.id";
/// Số chiều nó khai, nếu nó khai. Guard thứ hai — xem [`crate::library`].
pub const META_EMBEDDER_DIM: &str = "embedder.dim";
/// Bộ nhúng **trước đó**, ghi lúc xoá vector và xoá đi khi nhúng lại xong. Đây là thứ
/// khiến `stats()` nói được "đang nhúng lại vì đổi mô hình" thay vì "đang xếp hàng".
pub const META_EMBEDDER_PREVIOUS: &str = "embedder.previous";

/// Một hàng `documents` như kho thấy nó.
#[derive(Clone, Debug)]
pub struct DocumentRow {
    pub id: String,
    pub path: PathBuf,
    pub origin: String,
    pub title: String,
    pub format: Format,
    pub bytes: u64,
    pub added_at: i64,
    pub error: Option<String>,
    pub chunks: u32,
    /// Mọi đoạn của tài liệu đều đã có vector.
    pub embedded: bool,
}

/// Một hàng `chunks`, đủ để dựng một trích dẫn.
#[derive(Clone, Debug)]
pub struct ChunkRow {
    pub id: i64,
    pub document_id: String,
    pub title: String,
    pub path: PathBuf,
    pub ord: u32,
    pub heading: Option<String>,
    pub body: String,
}

/// Đếm tổng, cho `stats()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
}

/// Việc kho đã làm lúc mở tệp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// Tệp mới, hoặc tệp cũ còn đúng schema.
    Ready,
    /// Schema đã cũ và vừa bị dựng lại; bản sao tệp trong kho là thứ phải nạp lại.
    Rebuilt,
}

pub struct Store {
    /// `Connection` không `Sync`. Một khoá thật thay vì một pool: thư viện tài liệu là
    /// chỗ ghi tuần tự, và mọi lần giữ khoá đều nằm gọn trong một `spawn_blocking`.
    conn: Mutex<Connection>,
    opened: Opened,
}

impl Store {
    /// `files_dir` là thư mục chứa bản sao tệp. Kho cần biết nó để trả lời được câu hỏi
    /// "dựng lại chỉ mục thì có dựng lại được không" — xem [`ensure_schema`].
    pub fn open(path: &Path, files_dir: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| RagError::io(parent.display(), err))?;
        }
        Store::from_connection(
            Connection::open(path).map_err(|err| RagError::Store(err.to_string()))?,
            files_dir,
        )
    }

    /// Cho bài kiểm chứng, và cho phiên không cần sống qua lần khởi động sau.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(
            Connection::open_in_memory().map_err(|err| RagError::Store(err.to_string()))?,
            Path::new(""),
        )
    }

    fn from_connection(mut conn: Connection, files_dir: &Path) -> Result<Store> {
        configure(&conn)?;
        let opened = ensure_schema(&mut conn, files_dir)?;
        Ok(Store {
            conn: Mutex::new(conn),
            opened,
        })
    }

    pub fn opened(&self) -> Opened {
        self.opened
    }

    fn with<T>(&self, body: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| RagError::Unavailable("khoá kết nối kho tài liệu bị nhiễm độc".into()))?;
        body(&mut guard)
    }

    /// Tài liệu mang đúng nội dung này, nếu đã có.
    pub fn by_sha(&self, sha: &str) -> Result<Option<String>> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM documents WHERE sha256 = ?1",
                    params![sha],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// Ghi một tài liệu và toàn bộ đoạn của nó, trong một giao dịch.
    ///
    /// Cùng `sha256` thì **cập nhật**, không chèn: tài liệu giữ nguyên mã, nên mọi trích
    /// dẫn đã phát ra cho mô hình vẫn trỏ đúng chỗ. Đoạn thì thay sạch — một tài liệu
    /// được nạp lại có thể *mất* đoạn chứ không chỉ thêm, và lần ra cái đã mất tốn hơn là
    /// viết lại cả nhóm.
    #[allow(clippy::too_many_arguments)]
    pub fn put_document(
        &self,
        id: &str,
        path: &Path,
        origin: &str,
        title: &str,
        format: Format,
        sha: &str,
        bytes: u64,
        added_at: i64,
        chunks: &[Chunk],
    ) -> Result<()> {
        let path = path.display().to_string();
        self.with(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO documents \
                 (id, path, origin, title, format, sha256, bytes, added_at, error) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL) \
                 ON CONFLICT(sha256) DO UPDATE SET \
                 path = ?2, origin = ?3, title = ?4, format = ?5, bytes = ?7, error = NULL",
                params![
                    id,
                    path,
                    origin,
                    title,
                    format.as_str(),
                    sha,
                    bytes as i64,
                    added_at
                ],
            )?;
            let id: String = tx.query_row(
                "SELECT id FROM documents WHERE sha256 = ?1",
                params![sha],
                |row| row.get(0),
            )?;
            forget_chunks(&tx, &id)?;
            for chunk in chunks {
                tx.execute(
                    "INSERT INTO chunks (document_id, ord, heading, body, start_byte, end_byte) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        id,
                        chunk.ord,
                        chunk.heading,
                        chunk.text,
                        chunk.start as i64,
                        chunk.end as i64
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Ghi lý do phần ngữ nghĩa của tài liệu này chưa sẵn sàng.
    ///
    /// `None` là "xếp hàng, chưa nhúng" — khác hẳn `Some(lý do)`. Phía `app/` phân biệt
    /// đúng hai trạng thái đó trong `DocumentView.error`.
    pub fn set_error(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "UPDATE documents SET error = ?2 WHERE id = ?1",
                params![id, error],
            )?;
            Ok(())
        })
    }

    pub fn documents(&self) -> Result<Vec<DocumentRow>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT d.id, d.path, d.origin, d.title, d.format, d.bytes, d.added_at, d.error, \
                 (SELECT count(*) FROM chunks c WHERE c.document_id = d.id), \
                 (SELECT count(*) FROM chunks c LEFT JOIN vectors v ON v.chunk_id = c.id \
                  WHERE c.document_id = d.id AND v.chunk_id IS NULL) \
                 FROM documents d ORDER BY d.added_at DESC, d.title",
            )?;
            let rows = stmt.query_map([], |row| {
                let chunks: i64 = row.get(8)?;
                let missing: i64 = row.get(9)?;
                Ok(DocumentRow {
                    id: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    origin: row.get(2)?,
                    title: row.get(3)?,
                    format: Format::parse(&row.get::<_, String>(4)?),
                    bytes: row.get::<_, i64>(5)?.max(0) as u64,
                    added_at: row.get(6)?,
                    error: row.get(7)?,
                    chunks: chunks.max(0) as u32,
                    // Một tài liệu không có đoạn nào — tệp rỗng — không được coi là "đã
                    // nhúng": nó không có gì để tìm ra cả.
                    embedded: chunks > 0 && missing == 0,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Đoạn của một tài liệu, theo thứ tự đọc.
    pub fn chunks_of(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ChunkRow>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.document_id, d.title, d.path, c.ord, c.heading, c.body \
                 FROM chunks c JOIN documents d ON d.id = c.document_id \
                 WHERE c.document_id = ?1 ORDER BY c.ord LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(
                params![document_id, limit as i64, offset as i64],
                read_chunk,
            )?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn chunks_by_id(&self, ids: &[i64]) -> Result<Vec<ChunkRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.document_id, d.title, d.path, c.ord, c.heading, c.body \
                 FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.id = ?1",
            )?;
            let mut out = Vec::with_capacity(ids.len());
            // Một câu cho mỗi mã, vì `ids` đã được xếp hạng và thứ tự đó là kết quả của
            // cả phép hợp nhất — một `IN (…)` trả về theo thứ tự của SQLite và làm hỏng nó.
            for id in ids {
                if let Some(row) = stmt.query_row(params![id], read_chunk).optional()? {
                    out.push(row);
                }
            }
            Ok(out)
        })
    }

    /// Mã của những đoạn chưa có vector.
    pub fn chunks_without_vectors(&self, limit: usize) -> Result<Vec<(i64, String)>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.body FROM chunks c LEFT JOIN vectors v ON v.chunk_id = c.id \
                 WHERE v.chunk_id IS NULL ORDER BY c.id LIMIT ?1",
            )?;
            let rows =
                stmt.query_map(params![limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Ghi vector. `f32` little-endian, không có header — số chiều nằm ở cột `dim`, nên
    /// một vector đọc ra sai độ dài là một lỗi bắt được chứ không phải rác đọc tiếp.
    pub fn put_vectors(&self, rows: &[(i64, Vec<f32>)]) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            for (chunk_id, vector) in rows {
                let mut data = Vec::with_capacity(vector.len() * 4);
                for value in vector {
                    data.extend_from_slice(&value.to_le_bytes());
                }
                tx.execute(
                    "INSERT INTO vectors (chunk_id, dim, data) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(chunk_id) DO UPDATE SET dim = ?2, data = ?3",
                    params![chunk_id, vector.len() as i64, data],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Mọi vector, để quét cosine trong Rust. Xem [`crate::search`] về cái giá của nó.
    pub fn all_vectors(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT chunk_id, dim, data FROM vectors")?;
            let rows = stmt.query_map([], |row| {
                let id: i64 = row.get(0)?;
                let dim: i64 = row.get(1)?;
                let data: Vec<u8> = row.get(2)?;
                Ok((id, dim as usize, data))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, dim, data) = row?;
                if data.len() != dim * 4 {
                    // Một hàng lệch chỉ có thể đến từ một bản cũ hơn hoặc một tệp hỏng.
                    // Bỏ qua nó và đi tiếp: mất một đoạn khỏi phần ngữ nghĩa nhẹ hơn nhiều
                    // so với làm hỏng cả lần tìm.
                    tracing::warn!(chunk = id, dim, bytes = data.len(), "vector lệch độ dài");
                    continue;
                }
                let values = data
                    .chunks_exact(4)
                    .map(|word| f32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                    .collect();
                out.push((id, values));
            }
            Ok(out)
        })
    }

    /// Tìm bằng FTS5. Trả về mã đoạn theo thứ tự BM25, tốt nhất trước.
    pub fn search_keyword(&self, query: &str, limit: usize) -> Result<Vec<i64>> {
        let Some((strict, loose)) = fts_expressions(query) else {
            return Ok(Vec::new());
        };
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1 \
                 ORDER BY bm25(chunks_fts, 1.0, 2.0) LIMIT ?2",
            )?;
            let mut hits = {
                let rows = stmt.query_map(params![strict, limit as i64], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<Vec<i64>>>()?
            };
            if hits.is_empty() {
                // Mọi từ cùng có mặt là phép lọc đúng khi nó trả về cái gì đó. Khi nó trả
                // về rỗng — người dùng gõ cả một câu hỏi chứ không phải một cụm từ khoá —
                // thì "có từ nào cũng được" còn hơn là không có gì.
                let rows = stmt.query_map(params![loose, limit as i64], |row| row.get(0))?;
                hits = rows.collect::<rusqlite::Result<Vec<i64>>>()?;
            }
            Ok(hits)
        })
    }

    /// Xoá một tài liệu và mọi dấu vết của nó. Xem luật 3 ở đầu tệp về `ON DELETE CASCADE`.
    pub fn remove_document(&self, id: &str) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            let known: Option<i64> = tx
                .query_row("SELECT 1 FROM documents WHERE id = ?1", params![id], |row| {
                    row.get(0)
                })
                .optional()?;
            if known.is_none() {
                return Err(RagError::NotFound(id.to_string()));
            }
            // Thứ tự ngược chiều tham chiếu, và **hàng tài liệu xoá sau cùng**. Xoá nó
            // trước thì cascade dọn `chunks` giúp ta — nhưng dọn mà không kích hoạt
            // trigger, nên chỉ mục FTS ở lại với những hàng trỏ vào đoạn không còn tồn tại.
            tx.execute(
                "DELETE FROM vectors WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?1)",
                params![id],
            )?;
            forget_chunks(&tx, id)?;
            tx.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn counts(&self) -> Result<Counts> {
        self.with(|conn| {
            Ok(Counts {
                documents: conn
                    .query_row("SELECT count(*) FROM documents", [], |r| r.get::<_, i64>(0))?
                    as u32,
                chunks: conn.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?
                    as u32,
                embedded_chunks: conn
                    .query_row("SELECT count(*) FROM vectors", [], |r| r.get::<_, i64>(0))?
                    as u32,
            })
        })
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn clear_meta(&self, key: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
            Ok(())
        })
    }

    /// Xoá sạch bảng `vectors`, giữ nguyên `documents` và `chunks`. Trả về số hàng đã xoá.
    ///
    /// **Chỉ vector, và đó là toàn bộ ý nghĩa của hàm này.** Đổi mô hình nhúng làm mọi
    /// vector cũ vô giá trị — cosine giữa hai không gian nhúng khác nhau là một con số vô
    /// nghĩa trông y hệt một con số có nghĩa — nhưng nó **không** làm chữ trong tài liệu
    /// vô giá trị. Xoá kèm `chunks` là làm hỏng FTS5 trong suốt thời gian nhúng lại, đúng
    /// lúc FTS5 là thứ duy nhất còn trả lời được.
    pub fn forget_vectors(&self) -> Result<usize> {
        self.with(|conn| Ok(conn.execute("DELETE FROM vectors", [])?))
    }

    /// Xoá lý do hỏng trên mọi tài liệu.
    ///
    /// Gọi lúc đổi bộ nhúng: "máy chủ cũ không trả lời" là một câu nói về máy chủ đã bị
    /// thay, và để nó ở lại thì `stats()` sẽ đổ lỗi cho đúng thứ không còn liên quan.
    pub fn clear_errors(&self) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "UPDATE documents SET error = NULL WHERE error IS NOT NULL",
                [],
            )?;
            Ok(())
        })
    }

    /// Lý do gần nhất mà một tài liệu chưa nhúng được.
    ///
    /// Một lý do là đủ cho `stats().reason`: khi bộ nhúng tắt thì cả hai mươi tài liệu
    /// mang đúng cùng một câu, và in ra hai mươi lần không nói thêm điều gì.
    pub fn first_error(&self) -> Result<Option<String>> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT error FROM documents WHERE error IS NOT NULL \
                     ORDER BY added_at DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// Bao nhiêu đoạn khớp một cụm từ, hỏi thẳng chỉ mục FTS.
    ///
    /// Có mặt vì bài kiểm chứng cần khẳng định bằng **số hàng**, không phải bằng lời:
    /// đếm `chunks_fts` bằng `count(*)` thì SQLite đi đọc bảng nội dung và không nói gì
    /// về chỉ mục cả, nên nó sẽ bỏ lọt đúng cái lỗi hàng FTS mồ côi.
    pub fn count_keyword_matches(&self, phrase: &str) -> Result<i64> {
        let Some((strict, _)) = fts_expressions(phrase) else {
            return Ok(0);
        };
        self.with(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                params![strict],
                |row| row.get(0),
            )?)
        })
    }

    /// Chỉ mục FTS còn khớp với bảng nội dung không. Lỗi là câu trả lời "không".
    pub fn fts_integrity(&self) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO chunks_fts (chunks_fts) VALUES ('integrity-check')",
                [],
            )?;
            Ok(())
        })
    }

    /// Gộp WAL vào tệp chính. Gọi lúc tháo plugin để thư mục dự án không ở lại với một
    /// tệp `-wal` mà lần mở sau phải phát lại.
    pub fn checkpoint(&self) -> Result<()> {
        self.with(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
    }
}

fn read_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkRow> {
    Ok(ChunkRow {
        id: row.get(0)?,
        document_id: row.get(1)?,
        title: row.get(2)?,
        path: PathBuf::from(row.get::<_, String>(3)?),
        ord: row.get::<_, i64>(4)?.max(0) as u32,
        heading: row.get(5)?,
        body: row.get(6)?,
    })
}

fn forget_chunks(tx: &rusqlite::Transaction<'_>, document_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM chunks WHERE document_id = ?1",
        params![document_id],
    )?;
    Ok(())
}

/// Câu hỏi của người dùng → hai biểu thức FTS5 an toàn: chặt (AND) và lỏng (OR).
///
/// Chuỗi người dùng **không bao giờ** được ghép thẳng vào cú pháp `MATCH`: `"`, `*`, `:`,
/// `^`, `NOT`, `NEAR` đều có nghĩa ở đó, nên một câu hỏi bình thường có thể thành lỗi cú
/// pháp và một câu hỏi cố ý có thể thành một truy vấn khác hẳn. Cắt thành token rồi bọc
/// nháy kép biến mọi thứ thành chữ nghĩa thuần tuý.
fn fts_expressions(query: &str) -> Option<(String, String)> {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some((tokens.join(" AND "), tokens.join(" OR ")))
}

fn configure(conn: &Connection) -> Result<()> {
    // `trusted_schema` giữ nguyên mặc định — xem quyết định 1 ở đầu tệp. Tắt nó là làm
    // mọi lần chèn đoạn hỏng, vì trigger đồng bộ ghi vào một virtual table.
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| RagError::Store(err.to_string()))?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|err| RagError::Store(err.to_string()))?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        tracing::debug!(mode, "không bật được WAL cho kho tài liệu này");
    }
    // Nội dung gốc là bản sao tệp nằm ngay bên cạnh, nên một giao dịch mất vì mất điện là
    // một lần nạp lại chứ không phải mất dữ liệu. `NORMAL` là mức đúng cho thứ như thế.
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|err| RagError::Store(err.to_string()))?;
    Ok(())
}

/// Bảo đảm schema, và quyết định phải làm gì khi nó đã cũ.
///
/// `pai-index` dựng lại vô điều kiện, vì nguồn của nó là mã nguồn trong thư mục làm việc
/// và nó luôn ở đó. Ở đây nguồn là **bản sao tệp trong kho của dự án**, và nếu thư mục đó
/// đã bị xoá thì bảng `documents` là bản ghi duy nhất còn lại về việc người dùng từng nạp
/// những gì. Xoá nó đi để "dựng lại" là xoá đúng cái không dựng lại được.
///
/// Nên: dựng lại khi thư viện đang rỗng, hoặc khi bản sao tệp vẫn còn trong kho. Ngoài
/// hai trường hợp đó thì từ chối mở và nói ra lý do — người dùng còn đường chép tệp về
/// trước khi mất chúng. Từ chối làm việc là câu trả lời tệ, nhưng xoá dữ liệu của người
/// khác trong im lặng là câu trả lời tệ hơn.
fn ensure_schema(conn: &mut Connection, files_dir: &Path) -> Result<Opened> {
    let app_id: i32 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let populated: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'documents'",
        [],
        |row| row.get(0),
    )?;

    let mut opened = Opened::Ready;
    if populated > 0 {
        if app_id != APPLICATION_ID {
            return Err(RagError::Store(
                "tệp này không phải kho thư viện tài liệu; từ chối ghi đè".into(),
            ));
        }
        if version == SCHEMA_VERSION {
            return Ok(Opened::Ready);
        }
        let documents: i64 =
            conn.query_row("SELECT count(*) FROM documents", [], |row| row.get(0))?;
        if documents > 0 && !has_files(files_dir) {
            return Err(RagError::Store(format!(
                "schema thư viện đã cũ (bản {version}, cần {SCHEMA_VERSION}) nhưng kho tệp \
                 {} trống nên không dựng lại được; hãy chép tệp về rồi nạp lại",
                files_dir.display()
            )));
        }
        tracing::info!(
            from = version,
            to = SCHEMA_VERSION,
            documents,
            "schema thư viện đã cũ, dựng lại từ bản sao tệp trong kho"
        );
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TRIGGER IF EXISTS chunks_after_insert; \
             DROP TRIGGER IF EXISTS chunks_after_delete; \
             DROP TRIGGER IF EXISTS chunks_after_update; \
             DROP TABLE IF EXISTS chunks_fts; \
             DROP TABLE IF EXISTS vectors; \
             DROP TABLE IF EXISTS chunks; \
             DROP TABLE IF EXISTS documents; \
             DROP TABLE IF EXISTS meta;",
        )?;
        tx.commit()?;
        opened = Opened::Rebuilt;
    }

    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.commit()?;
    conn.pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(|err| RagError::Store(err.to_string()))?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|err| RagError::Store(err.to_string()))?;
    Ok(opened)
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.any(|entry| entry.is_ok()))
        .unwrap_or(false)
}
