//! Kho: SQLite + FTS5, và một đồ thị dựng trên chính bảng ký hiệu ấy.
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
//!
//! # Đồ thị: hai bảng chứ không một
//!
//! `refs` giữ cái **nhìn thấy được trong một tệp**: chỗ này nhắc tới cái tên kia. `edges`
//! giữ cái **đã phân giải**: chỗ này nối vào ký hiệu số kia. Tách ra vì hai thứ có vòng
//! đời khác nhau — `refs` chỉ đổi khi tệp đổi, còn `edges` đổi mỗi khi **bất kỳ** tệp nào
//! đổi: `helper()` trong `a.rs` không nối được vào đâu cho tới lúc `b.rs` được quét, và
//! nếu chỉ có một bảng thì cạnh đó phải chờ tới lần `a.rs` được sửa mới xuất hiện — tức
//! là một đồ thị đúng hay sai tuỳ theo thứ tự đi thư mục.
//!
//! Cả hai bảng treo vào `symbols(id)` bằng `ON DELETE CASCADE`. Đó không phải tiện lợi:
//! quét lại một tệp đã sửa mà cạnh cũ của nó còn ở lại thì đồ thị lớn dần bằng rác, và
//! rác trong đồ thị trông y hệt sự thật.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::error::IndexError;
use crate::extract::Extraction;
use crate::graph::{
    CentralSymbol, DirectorySummary, EdgeKind, GraphEdge, GraphNode, MODULE_KIND, Overview, Owner,
    Stats, Target,
};
use crate::symbol::{Symbol, SymbolKind};

type Result<T> = std::result::Result<T, IndexError>;

/// `'PIDX'`. Mở nhầm một tệp SQLite khác thì thấy ngay, trước khi ghi vào.
const APPLICATION_ID: i32 = 0x50494458;
const SCHEMA_VERSION: i32 = 2;

/// Bao nhiêu ứng viên là còn đáng ghi.
///
/// Khi một cái tên vẫn còn nhiều khai báo sau cả bốn bậc ưu tiên, tất cả được ghi chứ
/// không chọn bừa một cái — xem [`crate::graph`]. Nhưng "tất cả" phải có trần: một tên
/// như `new` có hàng trăm khai báo trong một repo Rust, và hàng trăm cạnh từ một chỗ gọi
/// không thu hẹp được gì cho người đọc, nó chỉ làm mọi đỉnh nối với mọi đỉnh. Quá trần
/// thì bỏ hẳn tham chiếu đó: một cạnh không nói gì tệ hơn là không có cạnh.
const MAX_CANDIDATES: usize = 4;

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

CREATE TABLE refs (
  src      INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  kind     TEXT    NOT NULL,
  dst      INTEGER          REFERENCES symbols(id) ON DELETE CASCADE,
  dst_name TEXT,
  line     INTEGER NOT NULL
) STRICT;

CREATE INDEX refs_by_src ON refs (src);

CREATE TABLE edges (
  src  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  dst  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  kind TEXT    NOT NULL,
  path TEXT    NOT NULL,
  line INTEGER NOT NULL
) STRICT;

CREATE INDEX edges_by_src ON edges (src);
CREATE INDEX edges_by_dst ON edges (dst);
CREATE UNIQUE INDEX edges_once ON edges (src, dst, kind, line);

CREATE TABLE meta (
  key   TEXT    PRIMARY KEY,
  value INTEGER NOT NULL
) STRICT;
"#;

/// Đỉnh module không phải một khai báo mà người ta đi tìm, nên nó bị chặn khỏi mọi câu
/// hỏi của `symbol_search` và `outline`. Nó chỉ tồn tại trong đồ thị — xem
/// [`crate::graph::MODULE_KIND`].
const NOT_MODULE: &str = "s.kind <> 'module'";

const SELECT_SYMBOL: &str = "SELECT s.name, s.kind, s.parent, s.start_line, s.end_line, \
     s.signature, f.path FROM symbols s JOIN files f ON f.id = s.file_id";

const SELECT_NODE: &str = "SELECT s.id, s.name, s.kind, f.path, s.start_line \
     FROM symbols s JOIN files f ON f.id = s.file_id";

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

    /// Đường dẫn của mọi tệp đã biết, để hoàn thành `@` trong ô soạn tin.
    ///
    /// Chỉ lấy cột `path`, không lấy dấu vân tay như [`Store::known_files`]: gõ thêm một
    /// ký tự là một lần gọi lại, và kéo cả `mtime` lẫn `size` về chỉ để vứt đi là trả giá
    /// ở đúng chỗ người dùng cảm thấy — giữa hai lần nhấn phím.
    ///
    /// Xếp theo đường dẫn để thứ tự ổn định giữa hai lần gọi. Việc chấm điểm nằm ở
    /// [`crate::complete`], không ở đây: SQL xếp theo chữ cái, còn thứ người ta muốn thấy
    /// trước là tệp có **tên** khớp, thứ SQL không nói được.
    pub fn paths(&self) -> Result<Vec<String>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT path FROM files ORDER BY path")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    /// Thay toàn bộ ký hiệu **và** tham chiếu của một tệp, trong một giao dịch.
    ///
    /// Thay chứ không vá: một tệp vừa sửa có thể mất ký hiệu chứ không chỉ thêm, và lần
    /// ra cái đã mất tốn hơn là viết lại cả nhóm. Cạnh đi theo bằng `ON DELETE CASCADE`,
    /// nên không có đường nào để một cạnh cũ sống sót qua một lần quét lại.
    pub fn replace_file(
        &self,
        path: &str,
        lang: &str,
        mtime: i64,
        size: i64,
        found: &Extraction,
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

            // Đỉnh module đi trước mọi ký hiệu: nó là chủ nhà mặc định cho `use` ở đầu
            // tệp và cho những khai báo không có cha, nên nó phải tồn tại trước khi có ai
            // cần trỏ vào nó. Nó **không** vào FTS — người ta tra tên hàm, không tra tên tệp.
            let module = module_name(path);
            tx.execute(
                "INSERT INTO symbols (file_id, name, kind, parent, start_line, end_line, signature) \
                 VALUES (?1, ?2, ?3, NULL, 1, 1, ?4)",
                params![file_id, module, MODULE_KIND, path],
            )?;
            let module_id = tx.last_insert_rowid();

            let mut ids = Vec::with_capacity(found.symbols.len());
            let mut by_name: HashMap<&str, i64> = HashMap::new();
            for symbol in &found.symbols {
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
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO symbols_fts (rowid, name, parent, signature) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, symbol.name, symbol.parent, symbol.signature],
                )?;
                ids.push(id);
                by_name.entry(symbol.name.as_str()).or_insert(id);
            }

            for reference in &found.refs {
                // `impl Foo` không tự mình là ký hiệu; chủ nhà thật của nó là `struct Foo`
                // **trong chính tệp này**. Không tìm thấy — vì `Foo` khai ở tệp khác —
                // thì chủ nhà lùi về đỉnh module: tệp này vẫn thật sự chứa khối đó.
                let src = match &reference.from {
                    Owner::Symbol(index) => ids.get(*index).copied().unwrap_or(module_id),
                    Owner::Scope(name) => by_name.get(name.as_str()).copied().unwrap_or(module_id),
                    Owner::File => module_id,
                };
                let (dst, dst_name) = match &reference.to {
                    Target::Symbol(index) => (ids.get(*index).copied(), None),
                    Target::Name(name) => (None, Some(name.as_str())),
                };
                tx.execute(
                    "INSERT INTO refs (src, kind, dst, dst_name, line) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![src, reference.kind.as_str(), dst, dst_name, reference.line],
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

    /// Dựng lại **toàn bộ** bảng cạnh từ `refs`.
    ///
    /// Toàn bộ, không phải phần chênh, và đó là một lựa chọn có giá: một tệp đổi thì cả
    /// kho được phân giải lại. Cái nó mua là thứ không mua được bằng cách khác — một cạnh
    /// từ `a.rs` sang một hàm vừa được thêm vào `b.rs` **xuất hiện ngay**, thay vì nằm chờ
    /// tới lần ai đó động vào `a.rs`. Phần đắt của một lần quét là parse, và parse vẫn
    /// tăng dần; phân giải chỉ là một lượt tra bảng băm trên vài nghìn hàng đã nằm trong RAM.
    ///
    /// Bậc ưu tiên: **cùng tệp → cùng thư mục → cùng ngôn ngữ → toàn kho**. Bậc đầu tiên
    /// có ứng viên là bậc thắng, và những bậc sau không được xét nữa — một `helper` ngay
    /// trong tệp gần như luôn là `helper` mà người viết đang nói tới, kể cả khi có mười
    /// cái cùng tên ở nơi khác.
    ///
    /// Trả về số cạnh đã ghi.
    pub fn rebuild_edges(&self) -> Result<usize> {
        self.with(|conn| {
            let tx = conn.transaction()?;

            let mut files: HashMap<i64, FileRow> = HashMap::new();
            {
                let mut stmt = tx.prepare("SELECT id, path, lang FROM files")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let id: i64 = row.get(0)?;
                    let path: String = row.get(1)?;
                    let dir = Path::new(&path)
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    files.insert(
                        id,
                        FileRow {
                            path,
                            dir,
                            lang: row.get(2)?,
                        },
                    );
                }
            }

            let mut by_name: HashMap<String, Vec<Candidate>> = HashMap::new();
            {
                let mut stmt = tx.prepare("SELECT id, name, kind, file_id FROM symbols")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let kind: String = row.get(2)?;
                    let name: String = row.get(1)?;
                    by_name.entry(name).or_default().push(Candidate {
                        id: row.get(0)?,
                        file: row.get(3)?,
                        module: kind == MODULE_KIND,
                    });
                }
            }

            tx.execute("DELETE FROM edges", [])?;
            let mut written = 0usize;
            {
                let mut insert = tx.prepare(
                    "INSERT OR IGNORE INTO edges (src, dst, kind, path, line) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                let mut stmt = tx.prepare(
                    "SELECT r.src, r.kind, r.dst, r.dst_name, r.line, s.file_id \
                     FROM refs r JOIN symbols s ON s.id = r.src",
                )?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let src: i64 = row.get(0)?;
                    let kind_text: String = row.get(1)?;
                    let Some(kind) = EdgeKind::parse(&kind_text) else {
                        continue;
                    };
                    let line: i64 = row.get(4)?;
                    let site: i64 = row.get(5)?;
                    let Some(file) = files.get(&site) else {
                        continue;
                    };

                    let exact: Option<i64> = row.get(2)?;
                    let targets: Vec<i64> = match exact {
                        Some(id) => vec![id],
                        None => {
                            let name: Option<String> = row.get(3)?;
                            let Some(name) = name else { continue };
                            match by_name.get(&name) {
                                Some(pool) => resolve(pool, kind, site, file, &files),
                                None => Vec::new(),
                            }
                        }
                    };

                    for dst in targets {
                        // Một cạnh trỏ vào chính nó không dẫn đi đâu: đệ quy là sự thật
                        // nhưng nó không trả lời được câu hỏi nào mà đồ thị này phục vụ,
                        // và nó biến mọi lần duyệt thành một vòng lặp phải canh.
                        if dst == src {
                            continue;
                        }
                        written += insert.execute(params![
                            src,
                            dst,
                            kind.as_str(),
                            file.path.as_str(),
                            line
                        ])?;
                    }
                }
            }
            tx.commit()?;
            Ok(written)
        })
    }

    /// Ghi lại thời điểm quét xong, epoch mili-giây.
    pub fn mark_scanned(&self, at: i64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('scanned_at', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                params![at],
            )?;
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
                     WHERE symbols_fts MATCH ?1 AND (?2 IS NULL OR s.kind = ?2) AND {NOT_MODULE} \
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
                 AND (?2 IS NULL OR s.kind = ?2) AND {NOT_MODULE} \
                 ORDER BY length(s.name), s.name LIMIT ?3"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![like_pattern(query), kind, limit], read_symbol)?;
            Ok(rows.collect::<rusqlite::Result<Vec<Symbol>>>()?)
        })
    }

    /// Bản đồ ký hiệu của một tệp, theo thứ tự xuất hiện.
    pub fn outline(&self, path: &str) -> Result<Vec<Symbol>> {
        self.with(|conn| {
            let sql = format!(
                "{SELECT_SYMBOL} WHERE f.path = ?1 AND {NOT_MODULE} \
                 ORDER BY s.start_line, s.end_line DESC"
            );
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
        self.with(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM symbols s WHERE s.kind <> 'module'",
                [],
                |r| r.get(0),
            )?)
        })
    }

    pub fn edge_count(&self) -> Result<i64> {
        self.with(|conn| Ok(conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0))?))
    }

    /// Đỉnh mang đúng cái tên này.
    ///
    /// `Foo::bar` được tách làm hai: mô hình chép lại nguyên cái tên đủ tư cách mà
    /// `symbol_search` vừa in ra cho nó, và bắt nó gõ lại chỉ nửa sau là bắt nó đoán.
    pub fn nodes_named(&self, name: &str) -> Result<Vec<GraphNode>> {
        let (parent, leaf) = match name.rsplit_once("::") {
            Some((parent, leaf)) => (Some(parent.to_string()), leaf.to_string()),
            None => (None, name.to_string()),
        };
        self.with(|conn| {
            let sql = format!(
                "{SELECT_NODE} WHERE s.name = ?1 AND (?2 IS NULL OR s.parent = ?2) \
                 AND {NOT_MODULE} ORDER BY f.path, s.start_line"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![leaf, parent], read_node)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphNode>>>()?)
        })
    }

    pub fn nodes_by_ids(&self, ids: &[i64]) -> Result<Vec<GraphNode>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let sql = format!("{SELECT_NODE} WHERE s.id IN ({})", placeholders(ids.len()));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids), read_node)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphNode>>>()?)
        })
    }

    /// Mọi cạnh chạm vào một trong các đỉnh này, cả hai chiều.
    ///
    /// Chiều ngược đi qua chỉ mục trên `dst`: câu hỏi "ai gọi hàm này" là câu hỏi hay
    /// nhất mà đồ thị trả lời được, và không có chỉ mục đó thì nó là một lần quét cả bảng.
    pub fn edges_touching(&self, ids: &[i64]) -> Result<Vec<GraphEdge>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let holes = placeholders(ids.len());
            let sql = format!(
                "SELECT src, dst, kind FROM edges WHERE src IN ({holes}) \
                 UNION SELECT src, dst, kind FROM edges WHERE dst IN ({holes})"
            );
            // Hai nửa của `UNION` dùng lại **đúng** những chỗ giữ `?1..?n` ấy, nên bộ
            // tham số chỉ được truyền một lần: SQLite đếm chỗ giữ riêng biệt, không đếm
            // số lần chúng xuất hiện.
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids), read_edge)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphEdge>>>()?)
        })
    }

    /// Láng giềng theo một loại cạnh và một chiều. `forward` là đi theo mũi tên.
    pub fn step(&self, ids: &[i64], kind: EdgeKind, forward: bool) -> Result<Vec<GraphEdge>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let column = if forward { "src" } else { "dst" };
            let sql = format!(
                "SELECT src, dst, kind FROM edges WHERE kind = ?1 AND {column} IN ({})",
                placeholders_from(ids.len(), 2)
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut args: Vec<rusqlite::types::Value> =
                vec![rusqlite::types::Value::Text(kind.as_str().to_string())];
            args.extend(ids.iter().map(|id| rusqlite::types::Value::from(*id)));
            let rows = stmt.query_map(params_from_iter(args), read_edge)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphEdge>>>()?)
        })
    }

    /// Cạnh quan sát được trong một tệp, đã kèm hai đầu. Dùng để kiểm chứng và để gỡ lỗi.
    pub fn edges_of_file(&self, path: &str) -> Result<Vec<(GraphNode, EdgeKind, GraphNode)>> {
        self.with(|conn| {
            let sql = "SELECT e.kind, \
                 a.id, a.name, a.kind, af.path, a.start_line, \
                 b.id, b.name, b.kind, bf.path, b.start_line \
                 FROM edges e \
                 JOIN symbols a ON a.id = e.src JOIN files af ON af.id = a.file_id \
                 JOIN symbols b ON b.id = e.dst JOIN files bf ON bf.id = b.file_id \
                 WHERE e.path = ?1 ORDER BY e.line, e.kind, b.name";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![path], |row| {
                let kind: String = row.get(0)?;
                Ok((
                    GraphNode {
                        id: row.get(1)?,
                        name: row.get(2)?,
                        kind: row.get(3)?,
                        path: row.get(4)?,
                        line: row.get(5)?,
                    },
                    EdgeKind::parse(&kind).unwrap_or(EdgeKind::References),
                    GraphNode {
                        id: row.get(6)?,
                        name: row.get(7)?,
                        kind: row.get(8)?,
                        path: row.get(9)?,
                        line: row.get(10)?,
                    },
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn stats(&self) -> Result<Stats> {
        self.with(|conn| {
            let scanned_at: Option<i64> = conn
                .query_row("SELECT value FROM meta WHERE key = 'scanned_at'", [], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(Stats {
                files: conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?,
                symbols: conn.query_row(
                    "SELECT count(*) FROM symbols s WHERE s.kind <> 'module'",
                    [],
                    |r| r.get(0),
                )?,
                edges: conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0))?,
                languages: languages(conn)?,
                scanned_at,
            })
        })
    }

    /// Bản đồ kiến trúc. `directories` bị cắt ở `dir_cap`, `central` ở `central_cap`.
    pub fn overview(&self, dir_cap: usize, central_cap: usize) -> Result<Overview> {
        self.with(|conn| {
            let mut folders: HashMap<String, DirectorySummary> = HashMap::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT f.path, (SELECT count(*) FROM symbols s \
                      WHERE s.file_id = f.id AND s.kind <> 'module') FROM files f",
                )?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let path: String = row.get(0)?;
                    let symbols: u32 = row.get(1)?;
                    let dir = Path::new(&path)
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    let entry = folders.entry(dir.clone()).or_insert(DirectorySummary {
                        path: dir,
                        files: 0,
                        symbols: 0,
                    });
                    entry.files += 1;
                    entry.symbols += symbols;
                }
            }
            let mut directories: Vec<DirectorySummary> = folders.into_values().collect();
            // Nhiều ký hiệu trước: một kho lạ được đọc từ chỗ đông nhất, không theo bảng chữ cái.
            directories.sort_by(|a, b| b.symbols.cmp(&a.symbols).then_with(|| a.path.cmp(&b.path)));
            let directories_omitted = directories.len().saturating_sub(dir_cap) as u32;
            directories.truncate(dir_cap);

            // `contains` bị loại khỏi phép đếm bậc: mọi tệp đều chứa ký hiệu của nó, nên
            // để nó vào thì thứ hạng chỉ nói lên tệp nào dài nhất — một câu `wc -l` đã
            // trả lời được và không ai cần một đồ thị để hỏi.
            let mut stmt = conn.prepare(
                "SELECT s.id, s.name, s.kind, f.path, s.start_line, d.incoming, d.outgoing \
                 FROM (SELECT id, sum(inc) AS incoming, sum(outg) AS outgoing FROM ( \
                         SELECT dst AS id, 1 AS inc, 0 AS outg FROM edges WHERE kind <> 'contains' \
                         UNION ALL \
                         SELECT src AS id, 0 AS inc, 1 AS outg FROM edges WHERE kind <> 'contains' \
                       ) GROUP BY id) d \
                 JOIN symbols s ON s.id = d.id JOIN files f ON f.id = s.file_id \
                 WHERE s.kind <> 'module' \
                 ORDER BY (d.incoming + d.outgoing) DESC, s.name LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![central_cap as i64], |row| {
                Ok(CentralSymbol {
                    node: GraphNode {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        kind: row.get(2)?,
                        path: row.get(3)?,
                        line: row.get(4)?,
                    },
                    incoming: row.get(5)?,
                    outgoing: row.get(6)?,
                })
            })?;
            let central = rows.collect::<rusqlite::Result<Vec<CentralSymbol>>>()?;

            Ok(Overview {
                directories,
                languages: languages(conn)?,
                central,
                directories_omitted,
            })
        })
    }
}

/// Một tệp, đã tách sẵn thư mục để khỏi cắt lại chuỗi cho từng tham chiếu.
struct FileRow {
    path: String,
    dir: String,
    lang: String,
}

#[derive(Clone, Copy)]
struct Candidate {
    id: i64,
    file: i64,
    module: bool,
}

/// Bốn bậc, và chỉ bậc đầu tiên có ứng viên mới được xét. Xem
/// [`Store::rebuild_edges`] để biết vì sao dừng ở bậc đầu tiên chứ không gộp.
fn resolve(
    pool: &[Candidate],
    kind: EdgeKind,
    site: i64,
    file: &FileRow,
    files: &HashMap<i64, FileRow>,
) -> Vec<i64> {
    let allowed: Vec<&Candidate> = pool
        .iter()
        .filter(|candidate| kind.may_target_module() || !candidate.module)
        .collect();
    if allowed.is_empty() {
        return Vec::new();
    }
    let tiers: [&dyn Fn(&Candidate) -> bool; 4] = [
        &|candidate: &Candidate| candidate.file == site,
        &|candidate: &Candidate| {
            files
                .get(&candidate.file)
                .is_some_and(|f| f.dir == file.dir)
        },
        &|candidate: &Candidate| {
            files
                .get(&candidate.file)
                .is_some_and(|f| f.lang == file.lang)
        },
        &|_: &Candidate| true,
    ];
    for tier in tiers {
        let hits: Vec<i64> = allowed
            .iter()
            .filter(|candidate| tier(candidate))
            .map(|candidate| candidate.id)
            .collect();
        if hits.is_empty() {
            continue;
        }
        return if hits.len() > MAX_CANDIDATES {
            Vec::new()
        } else {
            hits
        };
    }
    Vec::new()
}

/// Tên của đỉnh module: phần thân tên tệp. `store.rs` thành `store`, và đó cũng là cái
/// tên mà một `use crate::store::…` sẽ đi tra.
fn module_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn languages(conn: &Connection) -> rusqlite::Result<Vec<(String, u32)>> {
    let mut stmt =
        conn.prepare("SELECT lang, count(*) AS n FROM files GROUP BY lang ORDER BY n DESC, lang")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

fn placeholders(count: usize) -> String {
    placeholders_from(count, 1)
}

fn placeholders_from(count: usize, first: usize) -> String {
    (first..first + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn forget_symbols_of(tx: &rusqlite::Transaction<'_>, path: &str) -> Result<()> {
    // FTS trước, bảng thường sau: sau khi hàng `symbols` biến mất thì không còn cách nào
    // biết `rowid` nào cần xoá khỏi FTS, và một hàng FTS mồ côi vẫn trả về kết quả.
    tx.execute(
        "DELETE FROM symbols_fts WHERE rowid IN \
         (SELECT s.id FROM symbols s JOIN files f ON f.id = s.file_id WHERE f.path = ?1)",
        params![path],
    )?;
    // `refs` và `edges` đi theo bằng `ON DELETE CASCADE`, kể cả những cạnh **trỏ vào**
    // tệp này từ tệp khác. Cạnh đó sẽ được dựng lại ở lần phân giải kế tiếp nếu đích của
    // nó còn; nếu không còn thì nó vốn đã sai.
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

fn read_node(row: &Row<'_>) -> rusqlite::Result<GraphNode> {
    Ok(GraphNode {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        line: row.get(4)?,
    })
}

fn read_edge(row: &Row<'_>) -> rusqlite::Result<GraphEdge> {
    let kind: String = row.get(2)?;
    Ok(GraphEdge {
        src: row.get(0)?,
        dst: row.get(1)?,
        kind: EdgeKind::parse(&kind).unwrap_or(EdgeKind::References),
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
            "DROP TABLE IF EXISTS meta; \
             DROP TABLE IF EXISTS edges; \
             DROP TABLE IF EXISTS refs; \
             DROP TABLE IF EXISTS symbols_fts; \
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
