//! Kho: SQLite, FTS5, và một bảng vector.
//!
//! Một tệp cơ sở dữ liệu cho **mỗi dự án tài liệu**. Không dùng chung một kho cho mọi dự
//! án, vì triệu chứng của việc dùng chung là `docs.search` trả về một đoạn từ thư viện của
//! dự án khác — một câu trả lời trông y hệt một câu trả lời sai bình thường, nên không ai
//! lần ra nguyên nhân.
//!
//! Kho này là **chỉ mục soi vào thư mục dự án**, không phải bản gốc của gì cả. Chữ nằm
//! trong tệp của người dùng; ở đây chỉ có thứ suy ra được từ chúng. Đó là lý do mọi quyết
//! định bên dưới nghiêng về "dựng lại được" thay vì "giữ bằng mọi giá" — xem
//! `docs/CONTRACT.md`, luật 12.
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
//! **2. Đường dẫn là danh tính, `mtime` + kích thước là dấu vân tay.** Thư viện là thư
//! mục dự án, nên một hàng `documents` ứng với **một tệp đang nằm ở đó**, và hai tệp là
//! hai hàng kể cả khi nội dung giống hệt nhau — người dùng nhìn thấy hai tệp trong thư
//! mục của họ, và một danh sách hiện một hàng là danh sách nói dối. Bản trước băm nội dung
//! làm danh tính vì lúc ấy kho giữ **bản sao**; giờ không còn bản sao nào để gộp.
//!
//! Dấu vân tay thì đi theo `pai-index`: `mtime` + kích thước, không băm. Băm nội dung mọi
//! tệp ở mọi lần quét là đọc lại cả thư mục mỗi lần — đúng cái giá mà chỉ mục tăng dần
//! sinh ra để khỏi phải trả. Nó bỏ sót đúng một trường hợp: sửa tệp mà giữ nguyên cả độ
//! dài lẫn `mtime`, và cách duy nhất tạo ra nó là đặt lại `mtime` bằng tay.
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

use std::collections::{HashMap, HashSet};
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
/// vector đang nằm trong kho** — xem [`Store::forget_vectors`]. Bản 3 đổi danh tính tài
/// liệu từ băm nội dung sang **đường dẫn**, thêm `mtime` để quét tăng dần, và thêm bảng
/// `excluded` — cả ba đều là hệ quả của việc thư mục dự án trở thành thư viện.
const SCHEMA_VERSION: i32 = 3;

const SCHEMA: &str = r#"
CREATE TABLE documents (
  id        TEXT    PRIMARY KEY,
  path      TEXT    NOT NULL UNIQUE,
  origin    TEXT    NOT NULL,
  title     TEXT    NOT NULL,
  format    TEXT    NOT NULL,
  bytes     INTEGER NOT NULL,
  mtime     INTEGER NOT NULL,
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

-- Tệp còn nằm trong thư mục dự án nhưng người dùng đã bỏ khỏi thư viện.
--
-- Bảng này tồn tại vì `remove` không còn xoá tệp: không có nó thì lần quét ngay sau đó
-- nạp lại đúng cái tài liệu người dùng vừa bỏ đi, và một nút bấm không có tác dụng là
-- một nút bấm dạy người dùng rằng phần mềm không nghe lời họ.
CREATE TABLE excluded (
  path TEXT PRIMARY KEY,
  at   INTEGER NOT NULL
) STRICT;

-- Tệp thư viện đã thử đọc và không đọc được, kèm dấu vân tay lúc thử.
--
-- Không có bảng này thì mỗi lần quét lại đi rút chữ lại đúng những tệp đã hỏng — một PDF
-- cụt làm `pdf-extract` hoảng loạn ở mọi lần mở dự án — và bất biến "quét lại một thư mục
-- không đổi thì không rút chữ lại tệp nào" chỉ còn đúng với thư mục toàn tệp lành.
CREATE TABLE failures (
  path   TEXT    PRIMARY KEY,
  mtime  INTEGER NOT NULL,
  size   INTEGER NOT NULL,
  reason TEXT    NOT NULL
) STRICT;
"#;

/// Danh tính bộ nhúng đã sinh ra những vector đang nằm trong kho.
pub const META_EMBEDDER_ID: &str = "embedder.id";
/// Số chiều nó khai, nếu nó khai. Guard thứ hai — xem [`crate::library`].
pub const META_EMBEDDER_DIM: &str = "embedder.dim";
/// Bộ nhúng **trước đó**, ghi lúc xoá vector và xoá đi khi nhúng lại xong. Đây là thứ
/// khiến `stats()` nói được "đang nhúng lại vì đổi mô hình" thay vì "đang xếp hàng".
pub const META_EMBEDDER_PREVIOUS: &str = "embedder.previous";
/// Số tệp lần quét gần nhất nhìn thấy trong thư mục dự án.
pub const META_SCAN_FILES: &str = "scan.files";
/// Số tệp lần quét gần nhất bỏ qua vì chạm trần.
pub const META_SCAN_SKIPPED: &str = "scan.skipped";
/// Lúc quét xong lần gần nhất, millis. Ghi vào kho chứ không giữ trong bộ nhớ: giao diện
/// phải nói được "quét lúc nào" ngay khi vừa mở ứng dụng, trước lần quét đầu tiên.
pub const META_SCAN_AT: &str = "scan.at";

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

/// Dấu vân tay của một tệp đã nạp — đủ để nói "không đổi", không đủ để nói "giống hệt".
/// Cùng phép so với `pai_index::store::FileState`, và cùng lý do.
#[derive(Clone, Copy, Debug)]
pub struct FileState {
    pub mtime: i64,
    pub size: i64,
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
    /// `root` là **thư mục tài liệu của người dùng**. Kho cần biết nó để trả lời được câu
    /// hỏi "dựng lại chỉ mục thì có dựng lại được không" — xem [`ensure_schema`].
    pub fn open(path: &Path, root: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| RagError::io(parent.display(), err))?;
        }
        Store::from_connection(
            Connection::open(path).map_err(|err| RagError::Store(err.to_string()))?,
            root,
        )
    }

    /// Cho bài kiểm chứng, và cho phiên không cần sống qua lần khởi động sau.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(
            Connection::open_in_memory().map_err(|err| RagError::Store(err.to_string()))?,
            Path::new(""),
        )
    }

    fn from_connection(mut conn: Connection, root: &Path) -> Result<Store> {
        configure(&conn)?;
        let opened = ensure_schema(&mut conn, root)?;
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

    /// Tài liệu đang nằm ở đúng đường dẫn này, nếu đã có.
    pub fn by_path(&self, path: &str) -> Result<Option<String>> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM documents WHERE path = ?1",
                    params![path],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    /// Đường dẫn và dấu vân tay của mọi tài liệu đã nạp.
    ///
    /// Đây là **một nửa** của phép quét tăng dần; nửa kia là một loạt `stat` trên đĩa. Hai
    /// bên khớp nhau thì tệp không đi qua bộ rút chữ lần nào nữa — xem [`crate::library`].
    pub fn known_files(&self) -> Result<HashMap<String, FileState>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT path, mtime, bytes FROM documents")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileState {
                        mtime: row.get(1)?,
                        size: row.get(2)?,
                    },
                ))
            })?;
            let mut known = HashMap::new();
            for row in rows {
                let (path, state) = row?;
                known.insert(path, state);
            }
            Ok(known)
        })
    }

    /// Ghi một tài liệu và toàn bộ đoạn của nó, trong một giao dịch.
    ///
    /// Cùng đường dẫn thì **cập nhật**, không chèn: tài liệu giữ nguyên mã, nên mọi trích
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
        bytes: u64,
        mtime: i64,
        added_at: i64,
        chunks: &[Chunk],
    ) -> Result<()> {
        let path = path.display().to_string();
        self.with(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO documents \
                 (id, path, origin, title, format, bytes, mtime, added_at, error) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL) \
                 ON CONFLICT(path) DO UPDATE SET \
                 origin = ?3, title = ?4, format = ?5, bytes = ?6, mtime = ?7, error = NULL",
                params![
                    id,
                    path,
                    origin,
                    title,
                    format.as_str(),
                    bytes as i64,
                    mtime,
                    added_at
                ],
            )?;
            let id: String = tx.query_row(
                "SELECT id FROM documents WHERE path = ?1",
                params![path],
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

    /// Quên những tài liệu nằm ở các đường dẫn này. Trả về số hàng đã đi.
    ///
    /// Đường dẫn chứ không phải mã, vì người gọi là lần quét: nó biết **tệp nào đã biến
    /// mất khỏi đĩa**, không biết mã của chúng. Cùng thứ tự xoá với
    /// [`Store::remove_document`], và vì đúng cùng một lý do.
    pub fn forget_paths(&self, paths: &[String]) -> Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        self.with(|conn| {
            let tx = conn.transaction()?;
            let mut gone = 0usize;
            for path in paths {
                let id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM documents WHERE path = ?1",
                        params![path],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(id) = id else { continue };
                tx.execute(
                    "DELETE FROM vectors WHERE chunk_id IN \
                     (SELECT id FROM chunks WHERE document_id = ?1)",
                    params![id],
                )?;
                forget_chunks(&tx, &id)?;
                tx.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
                gone += 1;
            }
            tx.commit()?;
            Ok(gone)
        })
    }

    /// Ghi nhận rằng người dùng đã bỏ tệp này khỏi thư viện dù nó còn trên đĩa.
    pub fn exclude(&self, path: &str, at: i64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO excluded (path, at) VALUES (?1, ?2) \
                 ON CONFLICT(path) DO UPDATE SET at = ?2",
                params![path, at],
            )?;
            Ok(())
        })
    }

    /// Gỡ một đường dẫn khỏi danh sách loại trừ.
    ///
    /// Gọi khi người dùng **tự tay** nạp lại đúng tệp đó: một lời nói sau đè lên một lời
    /// nói trước, chứ không phải một nút bấm im lặng không có tác dụng.
    pub fn allow(&self, path: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM excluded WHERE path = ?1", params![path])?;
            Ok(())
        })
    }

    pub fn excluded(&self) -> Result<HashSet<String>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT path FROM excluded")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<rusqlite::Result<HashSet<String>>>()?)
        })
    }

    /// Dấu vân tay của những tệp đã thử đọc và không đọc được.
    ///
    /// Lần quét so bảng này y như so `documents`: khớp thì bỏ qua, không mở tệp ra nữa.
    /// Sửa tệp — `mtime` đổi — là lời mời thử lại, và đó đúng là điều người dùng vừa làm.
    pub fn failures(&self) -> Result<HashMap<String, FileState>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT path, mtime, size FROM failures")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileState {
                        mtime: row.get(1)?,
                        size: row.get(2)?,
                    },
                ))
            })?;
            let mut out = HashMap::new();
            for row in rows {
                let (path, state) = row?;
                out.insert(path, state);
            }
            Ok(out)
        })
    }

    pub fn put_failure(&self, path: &str, mtime: i64, size: i64, reason: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO failures (path, mtime, size, reason) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(path) DO UPDATE SET mtime = ?2, size = ?3, reason = ?4",
                params![path, mtime, size, reason],
            )?;
            Ok(())
        })
    }

    pub fn clear_failure(&self, path: &str) -> Result<()> {
        self.with(|conn| {
            conn.execute("DELETE FROM failures WHERE path = ?1", params![path])?;
            Ok(())
        })
    }

    pub fn forget_failures(&self, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.with(|conn| {
            let tx = conn.transaction()?;
            for path in paths {
                tx.execute("DELETE FROM failures WHERE path = ?1", params![path])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Bao nhiêu tệp trong thư mục dự án thư viện không đọc được, và bao nhiêu tệp người
    /// dùng đã bỏ ra. Hai con số này đi thẳng lên [`crate::Stats`]: một thư viện có ít tài
    /// liệu hơn số tệp trong thư mục phải nói được vì sao.
    pub fn side_counts(&self) -> Result<(u32, u32)> {
        self.with(|conn| {
            let failures: i64 =
                conn.query_row("SELECT count(*) FROM failures", [], |row| row.get(0))?;
            let excluded: i64 =
                conn.query_row("SELECT count(*) FROM excluded", [], |row| row.get(0))?;
            Ok((failures.max(0) as u32, excluded.max(0) as u32))
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
    // Nội dung gốc là tệp trong thư mục dự án, nên một giao dịch mất vì mất điện là một
    // lần quét lại chứ không phải mất dữ liệu. `NORMAL` là mức đúng cho thứ như thế.
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|err| RagError::Store(err.to_string()))?;
    Ok(())
}

/// Bảo đảm schema, và quyết định phải làm gì khi nó đã cũ.
///
/// Nguồn của kho này là **thư mục dự án**, y như nguồn của `pai-index` là thư mục làm
/// việc. Khi thư mục ấy đọc được thì mọi hàng ở đây đều dựng lại được trong một lần quét,
/// nên lệch schema là chuyện dựng lại chứ không phải chuyện mất mát.
///
/// Trường hợp còn lại mới là chỗ phải cẩn thận: thư mục **không đọc được** — ổ ngoài chưa
/// cắm, thư mục mạng chưa nối, thư mục vừa bị đổi tên. Lúc đó xoá bảng đi để "dựng lại" là
/// dựng lại ra một thư viện rỗng, và người dùng mở dự án lên thấy 0 tài liệu mà không có
/// lời giải thích nào — đúng cái lỗi mà cả đợt thay đổi này sinh ra để sửa. Nên: từ chối
/// mở, và nói ra thư mục nào không đọc được.
fn ensure_schema(conn: &mut Connection, root: &Path) -> Result<Opened> {
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
        if documents > 0 && !readable(root) {
            return Err(RagError::Store(format!(
                "schema thư viện đã cũ (bản {version}, cần {SCHEMA_VERSION}) nhưng thư mục dự \
                 án {} không đọc được nên không dựng lại được; hãy nối lại thư mục rồi mở lại",
                root.display()
            )));
        }
        tracing::info!(
            from = version,
            to = SCHEMA_VERSION,
            documents,
            root = %root.display(),
            "schema thư viện đã cũ, dựng lại bằng một lần quét thư mục dự án"
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
             DROP TABLE IF EXISTS excluded; \
             DROP TABLE IF EXISTS failures; \
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

/// Thư mục có mở ra đọc được không. Một thư mục **rỗng** vẫn là đọc được: người dùng dọn
/// hết tệp đi là một sự thật, còn ổ đĩa chưa cắm là một sự thật khác hẳn.
fn readable(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok()
}
