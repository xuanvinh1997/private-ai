//! Kho: SQLite + FTS5.
//!
//! Ba quyết định, và cả ba đều là chỗ bản Python thua.
//!
//! **1. FTS5.** Bản cũ quét toàn bộ mỗi lần hỏi vì nó không có chỉ mục đảo ngược nào cả.
//! Một bảng FTS5 đổi việc đó lấy một lần tra `MATCH`. Bảng FTS ở đây **tự giữ nội dung**
//! chứ không dùng `content=`: bảng external-content bắt buộc phải có trigger đồng bộ, mà
//! trigger ghi vào một virtual table là đúng thứ `trusted_schema = OFF` chặn. Đổi lại là
//! vài chục byte lặp cho mỗi ký hiệu — cái giá đúng để không phải chọn giữa tìm nhanh và
//! đóng một cửa.
//!
//! **2. `mtime` + kích thước.** Xem [`Store::known_files`]. Không có nó thì mỗi lần hỏi
//! là một lần parse lại cả repo, và tính năng bị người dùng tắt đi trước khi nó kịp có
//! ích.
//!
//! **3. Lệch schema thì dựng lại, không từ chối.** Ngược hẳn với sổ tay phiên, và vì một
//! lý do: sổ phiên là **nguồn sự thật**, mất là mất hẳn; chỉ mục thì derive được từ mã
//! nguồn trong vài giây. Từ chối mở một bộ nhớ đệm là từ chối làm việc mà không đổi lại
//! được gì. Chỉ khi tệp *không phải* của chỉ mục thì mới từ chối — lúc đó nó là dữ liệu
//! của người khác.

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::IndexError;
use crate::symbol::{Symbol, SymbolKind};

type Result<T> = std::result::Result<T, IndexError>;

/// `'PIDX'`. Mở nhầm một tệp SQLite khác thì thấy ngay, trước khi ghi vào.
const APPLICATION_ID: i32 = 0x50494458;
const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = r#"
CREATE TABLE files (
  id    INTEGER PRIMARY KEY,
  path  TEXT    NOT NULL UNIQUE,
  lang  TEXT    NOT NULL,
  mtime INTEGER NOT NULL,
  size  INTEGER NOT NULL
) STRICT;

CREATE TABLE symbols (
  id         INTEGER PRIMARY KEY,
  file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  name       TEXT    NOT NULL,
  kind       TEXT    NOT NULL,
  parent     TEXT,
  start_line INTEGER NOT NULL,
  end_line   INTEGER NOT NULL,
  signature  TEXT    NOT NULL
) STRICT;

CREATE INDEX symbols_by_file ON symbols (file_id, start_line);
CREATE INDEX symbols_by_name ON symbols (name);

CREATE VIRTUAL TABLE symbols_fts USING fts5(name, parent, signature, tokenize = 'unicode61');
"#;

const SELECT_SYMBOL: &str = "SELECT s.name, s.kind, s.parent, s.start_line, s.end_line, \
     s.signature, f.path FROM symbols s JOIN files f ON f.id = s.file_id";

/// Một hàng `files` đã biết, đủ để trả lời "tệp này có đổi không".
#[derive(Clone, Copy)]
pub struct FileState {
    pub id: i64,
    pub mtime: i64,
    pub size: i64,
}

pub struct Store {
    /// `Connection` không `Sync`. Một khoá thật thay vì một pool: chỉ mục là chỗ ghi
    /// tuần tự, và mọi lần giữ khoá đều nằm gọn trong một `spawn_blocking` của tầng trên.
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        Store::from_connection(Connection::open(path)?)
    }

    /// Cho bài kiểm chứng, và cho phiên không cần sống qua lần khởi động sau.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Store> {
        configure(&conn)?;
        ensure_schema(&mut conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, body: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| IndexError::Unavailable("khoá kết nối bị nhiễm độc".into()))?;
        body(&mut guard)
    }

    /// Mọi tệp đã biết, kèm dấu vân tay của chúng.
    ///
    /// Lấy hết một lần rồi so trong bộ nhớ, chứ không hỏi từng tệp một: một câu truy vấn
    /// cho mười nghìn tệp rẻ hơn mười nghìn câu, và bảng này vốn đã nằm gọn trong RAM.
    pub fn known_files(&self) -> Result<HashMap<String, FileState>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT id, path, mtime, size FROM files")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    FileState {
                        id: row.get(0)?,
                        mtime: row.get(2)?,
                        size: row.get(3)?,
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

    /// Thay toàn bộ ký hiệu của một tệp, trong một giao dịch.
    ///
    /// Thay chứ không vá: một tệp vừa sửa có thể mất ký hiệu chứ không chỉ thêm, và lần
    /// ra cái đã mất tốn hơn là viết lại cả nhóm.
    pub fn replace_file(
        &self,
        path: &str,
        lang: &str,
        mtime: i64,
        size: i64,
        symbols: &[Symbol],
    ) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            forget_symbols_of(&tx, path)?;
            tx.execute(
                "INSERT INTO files (path, lang, mtime, size) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(path) DO UPDATE SET lang = ?2, mtime = ?3, size = ?4",
                params![path, lang, mtime, size],
            )?;
            let file_id: i64 =
                tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                    r.get(0)
                })?;
            for symbol in symbols {
                tx.execute(
                    "INSERT INTO symbols \
                     (file_id, name, kind, parent, start_line, end_line, signature) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        file_id,
                        symbol.name,
                        symbol.kind.as_str(),
                        symbol.parent,
                        symbol.start_line,
                        symbol.end_line,
                        symbol.signature,
                    ],
                )?;
                tx.execute(
                    "INSERT INTO symbols_fts (rowid, name, parent, signature) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        tx.last_insert_rowid(),
                        symbol.name,
                        symbol.parent,
                        symbol.signature
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Quên hẳn một tệp: hàng `files`, ký hiệu của nó, và cả dấu vết trong FTS.
    pub fn forget_files(&self, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.with(|conn| {
            let tx = conn.transaction()?;
            for path in paths {
                forget_symbols_of(&tx, path)?;
                tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Tìm ký hiệu theo tên.
    ///
    /// Hai lượt, và lượt thứ hai không phải để chữa cháy. FTS5 cắt token ở ranh giới từ,
    /// nên `"handle"*` tìm ra `handleRequest` còn `"Request"` thì không — mà hỏi bằng nửa
    /// sau của một tên camelCase là chuyện người ta làm suốt. Lượt `LIKE` bù đúng chỗ đó,
    /// và nó chỉ chạy khi lượt đầu về rỗng nên không đánh đổi tốc độ của trường hợp
    /// thường.
    pub fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        let kind = kind.map(|k| k.as_str().to_string());
        let limit = limit as i64;
        self.with(|conn| {
            if let Some(expression) = fts_expression(query) {
                let sql = format!(
                    "{SELECT_SYMBOL} JOIN symbols_fts ON symbols_fts.rowid = s.id \
                     WHERE symbols_fts MATCH ?1 AND (?2 IS NULL OR s.kind = ?2) \
                     ORDER BY bm25(symbols_fts, 10.0, 3.0, 1.0), s.name LIMIT ?3"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![expression, kind, limit], read_symbol)?;
                let hits = rows.collect::<rusqlite::Result<Vec<Symbol>>>()?;
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
            let sql = format!(
                "{SELECT_SYMBOL} WHERE s.name LIKE ?1 ESCAPE '\\' \
                 AND (?2 IS NULL OR s.kind = ?2) ORDER BY length(s.name), s.name LIMIT ?3"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![like_pattern(query), kind, limit], read_symbol)?;
            Ok(rows.collect::<rusqlite::Result<Vec<Symbol>>>()?)
        })
    }

    /// Bản đồ ký hiệu của một tệp, theo thứ tự xuất hiện.
    pub fn outline(&self, path: &str) -> Result<Vec<Symbol>> {
        self.with(|conn| {
            let sql =
                format!("{SELECT_SYMBOL} WHERE f.path = ?1 ORDER BY s.start_line, s.end_line DESC");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![path], read_symbol)?;
            Ok(rows.collect::<rusqlite::Result<Vec<Symbol>>>()?)
        })
    }

    /// Tệp này đã có trong chỉ mục chưa. Dùng để phân biệt "tệp không có ký hiệu nào" với
    /// "tệp chưa từng được quét" — hai câu trả lời khác hẳn nhau đối với người hỏi.
    pub fn knows(&self, path: &str) -> Result<bool> {
        self.with(|conn| {
            let found: Option<i64> = conn
                .query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(found.is_some())
        })
    }

    pub fn symbol_count(&self) -> Result<i64> {
        self.with(|conn| Ok(conn.query_row("SELECT count(*) FROM symbols", [], |r| r.get(0))?))
    }
}

fn forget_symbols_of(tx: &rusqlite::Transaction<'_>, path: &str) -> Result<()> {
    // FTS trước, bảng thường sau: sau khi hàng `symbols` biến mất thì không còn cách nào
    // biết `rowid` nào cần xoá khỏi FTS, và một hàng FTS mồ côi vẫn trả về kết quả.
    tx.execute(
        "DELETE FROM symbols_fts WHERE rowid IN \
         (SELECT s.id FROM symbols s JOIN files f ON f.id = s.file_id WHERE f.path = ?1)",
        params![path],
    )?;
    tx.execute(
        "DELETE FROM symbols WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
        params![path],
    )?;
    Ok(())
}

fn read_symbol(row: &Row<'_>) -> rusqlite::Result<Symbol> {
    let kind: String = row.get(1)?;
    Ok(Symbol {
        name: row.get(0)?,
        // Một nhãn lạ trong cơ sở dữ liệu chỉ có thể đến từ một bản cũ hơn của chính
        // crate này; xếp nó vào `type` đọc được hơn là làm hỏng cả câu truy vấn.
        kind: SymbolKind::parse(&kind).unwrap_or(SymbolKind::Type),
        parent: row.get(2)?,
        start_line: row.get(3)?,
        end_line: row.get(4)?,
        signature: row.get(5)?,
        path: row.get(6)?,
    })
}

/// Biến câu hỏi của người dùng thành một biểu thức FTS5 an toàn.
///
/// Chuỗi của người dùng **không bao giờ** được ghép thẳng vào cú pháp MATCH: `"`, `*`,
/// `:`, `^`, `NOT` đều có nghĩa ở đó, nên một câu hỏi bình thường có thể thành lỗi cú
/// pháp, và một câu hỏi cố ý có thể thành một truy vấn khác hẳn. Cắt thành token rồi bọc
/// nháy kép biến mọi thứ thành chữ nghĩa thuần tuý; `*` cuối là của ta, không phải của họ.
fn fts_expression(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

/// `%` và `_` là ký tự đại diện của `LIKE`; thoát chúng thì `foo_bar` tìm đúng `foo_bar`.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        tracing::debug!(mode, "không bật được WAL cho kho chỉ mục này");
    }
    // Chỉ mục dựng lại được từ mã nguồn, nên một giao dịch mất vì mất điện không phải mất
    // dữ liệu — nó chỉ là một tệp phải parse lại. `NORMAL` là mức đúng cho một thứ như thế.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn ensure_schema(conn: &mut Connection) -> Result<()> {
    let app_id: i32 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let populated: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'files'",
        [],
        |row| row.get(0),
    )?;

    if populated > 0 {
        if app_id != APPLICATION_ID {
            return Err(IndexError::Store(
                "tệp này không phải kho chỉ mục; từ chối ghi đè".into(),
            ));
        }
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        tracing::info!(
            from = version,
            to = SCHEMA_VERSION,
            "schema chỉ mục đã cũ, dựng lại từ đầu"
        );
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS symbols_fts; \
             DROP TABLE IF EXISTS symbols; \
             DROP TABLE IF EXISTS files;",
        )?;
        tx.commit()?;
    }

    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.commit()?;
    conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
