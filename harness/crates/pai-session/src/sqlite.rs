//! Provider SQLite cho seam [`Sessions`].
//!
//! Ba quyết định định hình cả tệp này:
//!
//! 1. **`rusqlite` là đồng bộ.** Mọi lời gọi nằm trong `spawn_blocking`; không có đường
//!    nào từ đây chặn được runtime, kể cả khi ổ đĩa treo.
//! 2. **Gói mảnh stream.** Một mảnh delta cỡ một token, còn vỏ JSON của nó lớn gấp hàng
//!    chục lần. Một hàng cho mỗi mảnh là biến ổ đĩa thành nút cổ chai của việc gõ chữ,
//!    nên nhiều mảnh liên tiếp cùng một bước đi chung một hàng.
//! 3. **Không migrate ngầm.** Mở một tệp lạ hoặc lệch phiên bản schema là từ chối. Một
//!    lần migrate im lặng làm hỏng sổ thì không có cách nào dựng lại.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;

use crate::error::{Result, SessionError};
use crate::event::{AssistantMessage, Seq, SessionEvent, SessionEventEnvelope};
use crate::message::{ContentBlock, Message};
use crate::store::{NewSession, Origin, SessionHeader, SessionStore, new_session_id};
use crate::surface::SurfaceOp;

/// `'AGNT'`. Mở nhầm một tệp SQLite khác thì thấy ngay, thay vì thấy sau khi đã ghi vào.
const APPLICATION_ID: i32 = 0x41474E54;
const SCHEMA_VERSION: i32 = 1;

/// `data` là JSON thô, một hàng một sự kiện.
const ENC_JSON: i64 = 0;
/// `data` là một chùm mảnh stream, một hàng nhiều sự kiện.
const ENC_PACKED_CHUNKS: i64 = 2;

/// Không chứa `/`, nên nó không bao giờ lẫn được với một loại sự kiện thật. Hàng này
/// **không phải** một `SessionEvent`; nó là một chi tiết lưu trữ.
const PACKED_TYPE: &str = "assistant-chunks";

const SCHEMA: &str = r#"
CREATE TABLE persistence_state (
  singleton  INTEGER PRIMARY KEY CHECK (singleton = 1),
  store_id   TEXT    NOT NULL,
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions (
  id               INTEGER PRIMARY KEY,
  session_key      TEXT    NOT NULL UNIQUE,
  format_version   INTEGER NOT NULL,
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL,
  title            TEXT,
  cwd              TEXT,
  parent_session   TEXT,
  seed_length      INTEGER,
  origin           TEXT CHECK (origin IS NULL OR origin IN ('subagent')),
  delegation_depth INTEGER CHECK (delegation_depth IS NULL OR delegation_depth >= 0),
  agent_preset     TEXT,
  incarnation      TEXT    NOT NULL,
  revision         INTEGER NOT NULL DEFAULT 0,
  last_seq         INTEGER NOT NULL DEFAULT -1
) STRICT;

CREATE INDEX sessions_by_cwd     ON sessions (cwd, created_at DESC);
CREATE INDEX sessions_by_parent  ON sessions (parent_session);
CREATE INDEX sessions_by_created ON sessions (created_at DESC);

CREATE TABLE events (
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  seq               INTEGER NOT NULL CHECK (seq >= 0),
  type              TEXT    NOT NULL,
  time              INTEGER NOT NULL,
  data              ANY     NOT NULL,
  source_event_seqs ANY,
  surface_op        TEXT,
  ignorable         INTEGER CHECK (ignorable IS NULL OR ignorable IN (0,1)),
  enc               INTEGER NOT NULL DEFAULT 0 CHECK (enc IN (0,1,2)),
  PRIMARY KEY (session_id, seq)
) STRICT;

CREATE INDEX events_surface ON events (session_id, seq) WHERE surface_op IS NOT NULL;
CREATE INDEX events_by_type ON events (session_id, type, seq);
"#;

const SELECT_EVENTS: &str = "SELECT seq, type, time, data, source_event_seqs, surface_op, \
     ignorable, enc FROM events WHERE session_id = ?1 ORDER BY seq";

const INSERT_EVENT: &str = "INSERT INTO events \
     (session_id, seq, type, time, data, source_event_seqs, surface_op, ignorable, enc) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";

const SELECT_HEADER: &str = "SELECT session_key, format_version, created_at, updated_at, title, \
     cwd, parent_session, seed_length, origin, delegation_depth, agent_preset FROM sessions";

pub struct SqliteSessionStore {
    /// `Connection` không `Sync`. Một khoá thật thay vì một pool: sổ phiên là chỗ ghi
    /// tuần tự theo bản chất, và mọi lần giữ khoá đều nằm gọn trong một `spawn_blocking`.
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSessionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteSessionStore> {
        SqliteSessionStore::from_connection(Connection::open(path)?)
    }

    /// Cho bài kiểm chứng, và cho phiên không cần sống qua lần khởi động sau.
    pub fn open_in_memory() -> Result<SqliteSessionStore> {
        SqliteSessionStore::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<SqliteSessionStore> {
        configure(&conn)?;
        ensure_schema(&conn)?;
        Ok(SqliteSessionStore {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|_| SessionError::Unavailable("khoá kết nối bị nhiễm độc".into()))?;
            f(&mut guard)
        })
        .await;
        match joined {
            Ok(result) => result,
            Err(err) => Err(SessionError::Unavailable(err.to_string())),
        }
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    conn.pragma_update(None, "mmap_size", 0)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    // WAL cho phép đọc trong lúc ghi — giao diện phải cuộn được transcript giữa một
    // stream đang đổ về. Cơ sở dữ liệu trong bộ nhớ không có WAL; đó không phải lỗi.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        tracing::debug!(mode, "không bật được WAL cho kho phiên này");
    }
    // NORMAL chứ không FULL: với WAL, `NORMAL` chỉ đánh mất các giao dịch cuối khi cả
    // máy mất điện, không phải khi tiến trình chết. Đổi lại là một fsync mỗi lần ghi mảnh
    // stream — cái giá sai cho thứ đến vài chục lần một giây.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    let app_id: i32 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let user_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let has_tables: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'sessions'",
        [],
        |row| row.get(0),
    )?;

    if app_id == 0 && user_version == 0 && has_tables == 0 {
        conn.execute_batch(&format!("BEGIN IMMEDIATE;{SCHEMA}COMMIT;"))?;
        conn.execute(
            "INSERT INTO persistence_state (singleton, store_id, created_at) VALUES (1, ?1, ?2)",
            params![uuid::Uuid::now_v7().to_string(), now_ms()],
        )?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        return Ok(());
    }
    if app_id != APPLICATION_ID {
        return Err(SessionError::NotOurStore { found: app_id });
    }
    if user_version != SCHEMA_VERSION {
        return Err(SessionError::SchemaVersion {
            found: user_version,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// --- gói và mở gói mảnh stream ---------------------------------------------------------

/// Một hàng trong bảng `events`. Có thể đại diện một sự kiện, hoặc cả một chùm.
struct StoredRow {
    seq: i64,
    kind: String,
    time: i64,
    data: String,
    sources: Option<String>,
    surface_op: Option<String>,
    ignorable: Option<i64>,
    enc: i64,
}

/// Một mảnh chỉ gói được khi nó không mang gì ngoài payload: cờ và trích dẫn là dữ liệu
/// riêng của từng sự kiện, và một chùm không có chỗ để giữ chúng.
fn packable(envelope: &SessionEventEnvelope) -> bool {
    envelope.surface_op.is_none()
        && envelope.source_event_seqs.is_none()
        && envelope.ignorable.is_none()
}

/// Độ dài chuỗi mảnh liên tiếp bắt đầu tại `start` mà gói chung được.
fn chunk_run(events: &[SessionEventEnvelope], start: usize) -> usize {
    let SessionEvent::AssistantChunk(head) = &events[start].event else {
        return 0;
    };
    if !packable(&events[start]) {
        return 0;
    }
    let mut run = 1;
    while start + run < events.len() {
        let next = &events[start + run];
        let SessionEvent::AssistantChunk(chunk) = &next.event else {
            break;
        };
        let contiguous = next.seq == events[start + run - 1].seq + 1;
        if !packable(next) || !contiguous || chunk.turn != head.turn || chunk.step != head.step {
            break;
        }
        run += 1;
    }
    run
}

fn pack(events: &[SessionEventEnvelope]) -> Result<Vec<StoredRow>> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let run = chunk_run(events, i);
        if run >= 2 {
            rows.push(packed_row(&events[i..i + run])?);
            i += run;
        } else {
            rows.push(plain_row(&events[i])?);
            i += 1;
        }
    }
    Ok(rows)
}

fn plain_row(envelope: &SessionEventEnvelope) -> Result<StoredRow> {
    Ok(StoredRow {
        seq: envelope.seq as i64,
        kind: envelope.event.type_name().to_owned(),
        time: envelope.time,
        data: serde_json::to_string(&envelope.event.data()?)?,
        sources: envelope
            .source_event_seqs
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
        surface_op: envelope.surface_op.map(encode_surface_op).transpose()?,
        ignorable: envelope.ignorable.map(i64::from),
        enc: ENC_JSON,
    })
}

/// `dt[k]` là khoảng cách epoch-ms giữa hai mảnh liền nhau; nó có thể âm khi đồng hồ hệ
/// thống bị chỉnh. Lưu hiệu chứ không lưu mốc tuyệt đối vì hiệu là số nhỏ.
///
/// Các mảnh **không** được nối lại thành một chuỗi: ranh giới token là dữ liệu, và mất nó
/// là mất khả năng phát lại đúng cái giao diện đã hiển thị.
fn packed_row(run: &[SessionEventEnvelope]) -> Result<StoredRow> {
    let mut chunks = Vec::with_capacity(run.len());
    let mut dt = Vec::with_capacity(run.len().saturating_sub(1));
    let mut turn = 0;
    let mut step = 0;
    for (index, envelope) in run.iter().enumerate() {
        let SessionEvent::AssistantChunk(chunk) = &envelope.event else {
            return Err(SessionError::Unavailable("gói nhầm loại sự kiện".into()));
        };
        turn = chunk.turn;
        step = chunk.step;
        chunks.push(chunk.chunk.clone());
        if index > 0 {
            dt.push(envelope.time - run[index - 1].time);
        }
    }
    let data = serde_json::json!({ "turn": turn, "step": step, "dt": dt, "chunks": chunks });
    Ok(StoredRow {
        seq: run[0].seq as i64,
        kind: PACKED_TYPE.to_owned(),
        time: run[0].time,
        data: serde_json::to_string(&data)?,
        sources: None,
        surface_op: None,
        ignorable: None,
        enc: ENC_PACKED_CHUNKS,
    })
}

#[derive(serde::Deserialize)]
struct PackedChunks {
    turn: u64,
    step: u64,
    dt: Vec<i64>,
    chunks: Vec<Value>,
}

/// Mở gói. Kiểm hình dạng **trước** khi bung: một hàng gói hỏng phải kêu to, vì sản phẩm
/// của nó là những `seq` mà cả phần còn lại của hệ thống tin là liền mạch.
fn unpack(seq0: i64, time0: i64, data: &str) -> Result<Vec<SessionEventEnvelope>> {
    let packed: PackedChunks = serde_json::from_str(data)?;
    if !packed.chunks.is_empty() && packed.dt.len() + 1 != packed.chunks.len() {
        return Err(SessionError::Unavailable(format!(
            "hàng gói tại seq {seq0} có {} mảnh nhưng {} khoảng thời gian",
            packed.chunks.len(),
            packed.dt.len()
        )));
    }
    let mut time = time0;
    let mut out = Vec::with_capacity(packed.chunks.len());
    for (index, chunk) in packed.chunks.into_iter().enumerate() {
        if index > 0 {
            time += packed.dt[index - 1];
        }
        out.push(SessionEventEnvelope {
            seq: seq0 as Seq + index as Seq,
            time,
            event: SessionEvent::AssistantChunk(crate::event::AssistantChunk {
                turn: packed.turn,
                step: packed.step,
                chunk,
            }),
            ignorable: None,
            source_event_seqs: None,
            surface_op: None,
        });
    }
    Ok(out)
}

fn encode_surface_op(op: SurfaceOp) -> Result<String> {
    match op {
        // Chuỗi trần chứ không phải JSON có nháy: cột này còn để người đọc bằng mắt.
        SurfaceOp::Append => Ok("append".to_owned()),
        replace => Ok(serde_json::to_string(&replace)?),
    }
}

fn decode_surface_op(raw: &str) -> Result<SurfaceOp> {
    if raw == "append" {
        return Ok(SurfaceOp::Append);
    }
    Ok(serde_json::from_str(raw)?)
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<StoredRow> {
    Ok(StoredRow {
        seq: row.get(0)?,
        kind: row.get(1)?,
        time: row.get(2)?,
        data: row.get(3)?,
        sources: row.get(4)?,
        surface_op: row.get(5)?,
        ignorable: row.get(6)?,
        enc: row.get(7)?,
    })
}

fn expand(row: StoredRow) -> Result<Vec<SessionEventEnvelope>> {
    if row.enc == ENC_PACKED_CHUNKS {
        return unpack(row.seq, row.time, &row.data);
    }
    let data: Value = serde_json::from_str(&row.data)?;
    let sources: Option<Vec<Seq>> = row
        .sources
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let surface_op = row
        .surface_op
        .as_deref()
        .map(decode_surface_op)
        .transpose()?;
    let envelope = SessionEventEnvelope::from_parts(
        row.seq as Seq,
        row.time,
        &row.kind,
        data,
        row.ignorable.map(|flag| flag == 1),
        sources,
        surface_op,
    )?;
    Ok(vec![envelope])
}

/// Văn bản người đọc thấy được trong một sự kiện surface.
///
/// Đọc bằng chính kiểu của sổ chứ không bằng `json_extract` trong SQL: hình dạng payload
/// thuộc về `event.rs`, và một đường dẫn JSON viết trong chuỗi SQL là một bản sao thứ hai
/// của hình dạng đó — bản sao mà trình biên dịch không kiểm được.
fn preview_text(kind: &str, data: &str) -> Option<String> {
    let message = match kind {
        "user/message" => serde_json::from_str::<Message>(data).ok()?,
        "assistant/message" => serde_json::from_str::<AssistantMessage>(data).ok()?.message,
        _ => return None,
    };
    let text: String = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Cắt ở đây chứ không ở giao diện: một câu trả lời dài đi qua wire cho mỗi hàng trong
    // danh sách là băng thông trả cho thứ bị cắt ngay khi vẽ.
    let mut short: String = trimmed.chars().take(160).collect();
    if trimmed.chars().count() > 160 {
        short.push('…');
    }
    Some(short.replace(['\n', '\r'], " "))
}

fn header_from_row(row: &Row<'_>) -> rusqlite::Result<SessionHeader> {
    let origin: Option<String> = row.get(8)?;
    let seed_length: Option<i64> = row.get(7)?;
    let delegation_depth: Option<i64> = row.get(9)?;
    Ok(SessionHeader {
        id: row.get(0)?,
        format_version: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        title: row.get(4)?,
        cwd: row.get(5)?,
        parent_session: row.get(6)?,
        seed_length: seed_length.map(|n| n.max(0) as u64),
        origin: origin.as_deref().and_then(Origin::parse),
        delegation_depth: delegation_depth.map(|n| n.max(0) as u32),
        agent_preset: row.get(10)?,
    })
}

fn row_id(conn: &Connection, id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT id FROM sessions WHERE session_key = ?1",
        params![id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| SessionError::NotFound(id.to_owned()))
}

// --- provider ---------------------------------------------------------------------------

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, spec: NewSession) -> Result<SessionHeader> {
        let header = SessionHeader {
            id: spec.id.clone().unwrap_or_else(new_session_id),
            format_version: spec.format_version(),
            created_at: now_ms(),
            updated_at: now_ms(),
            title: None,
            cwd: spec.cwd,
            parent_session: spec.parent_session,
            seed_length: spec.seed_length,
            origin: spec.origin,
            delegation_depth: spec.delegation_depth,
            agent_preset: spec.agent_preset,
        };
        let row = header.clone();
        self.with_conn(move |conn| {
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM sessions WHERE session_key = ?1",
                    params![row.id],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.is_some() {
                return Err(SessionError::AlreadyExists(row.id));
            }
            conn.execute(
                "INSERT INTO sessions (session_key, format_version, created_at, updated_at, \
                 title, cwd, parent_session, seed_length, origin, delegation_depth, \
                 agent_preset, incarnation, revision, last_seq) \
                 VALUES (?1,?2,?3,?4,NULL,?5,?6,?7,?8,?9,?10,?11,0,-1)",
                params![
                    row.id,
                    row.format_version,
                    row.created_at,
                    row.updated_at,
                    row.cwd,
                    row.parent_session,
                    row.seed_length.map(|n| n as i64),
                    row.origin.map(Origin::as_str),
                    row.delegation_depth.map(i64::from),
                    row.agent_preset,
                    // Mới mỗi lần một tiến trình mở phiên: hai bản cùng ghi thì thấy ngay.
                    uuid::Uuid::now_v7().to_string(),
                ],
            )?;
            Ok(())
        })
        .await?;
        Ok(header)
    }

    async fn list(&self, limit: Option<u32>) -> Result<Vec<SessionHeader>> {
        self.with_conn(move |conn| {
            let sql = format!("{SELECT_HEADER} ORDER BY created_at DESC LIMIT ?1");
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map(params![limit.map_or(-1, i64::from)], header_from_row)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .await
    }

    async fn header(&self, id: &str) -> Result<SessionHeader> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            let sql = format!("{SELECT_HEADER} WHERE session_key = ?1");
            conn.query_row(&sql, params![id], header_from_row)
                .optional()?
                .ok_or(SessionError::NotFound(id))
        })
        .await
    }

    async fn append(&self, id: &str, events: Vec<SessionEventEnvelope>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let id = id.to_owned();
        self.with_conn(move |conn| {
            let tx = conn.transaction()?;
            let (session_id, last_seq): (i64, i64) = tx
                .query_row(
                    "SELECT id, last_seq FROM sessions WHERE session_key = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| SessionError::NotFound(id.clone()))?;

            // Chốt chặn cuối cho "seq liền mạch": lô phải nối đúng vào chỗ đang dở, và
            // bên trong lô cũng không được hở.
            let expected = (last_seq + 1) as Seq;
            if events[0].seq != expected {
                return Err(SessionError::SeqGap {
                    expected,
                    found: events[0].seq,
                });
            }
            for pair in events.windows(2) {
                if pair[1].seq != pair[0].seq + 1 {
                    return Err(SessionError::SeqGap {
                        expected: pair[0].seq + 1,
                        found: pair[1].seq,
                    });
                }
            }

            for row in pack(&events)? {
                let mut stmt = tx.prepare_cached(INSERT_EVENT)?;
                stmt.execute(params![
                    session_id,
                    row.seq,
                    row.kind,
                    row.time,
                    row.data,
                    row.sources,
                    row.surface_op,
                    row.ignorable,
                    row.enc,
                ])?;
            }

            let newest = events[events.len() - 1].seq as i64;
            let touched = tx.execute(
                "UPDATE sessions SET last_seq = ?2, revision = revision + 1, updated_at = ?3 \
                 WHERE id = ?1 AND last_seq = ?4",
                params![session_id, newest, now_ms(), last_seq],
            )?;
            if touched != 1 {
                return Err(SessionError::ConcurrentWrite(id));
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    async fn load(&self, id: &str) -> Result<Vec<SessionEventEnvelope>> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            let session_id = row_id(conn, &id)?;
            let mut stmt = conn.prepare_cached(SELECT_EVENTS)?;
            let rows = stmt.query_map(params![session_id], read_row)?;
            let mut out = Vec::new();
            for row in rows {
                out.extend(expand(row?)?);
            }
            Ok(out)
        })
        .await
    }

    async fn row_count(&self, id: &str) -> Result<u64> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            let session_id = row_id(conn, &id)?;
            let count: i64 = conn.query_row(
                "SELECT count(*) FROM events WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            Ok(count.max(0) as u64)
        })
        .await
    }

    async fn previews(&self, ids: &[String]) -> Result<HashMap<String, String>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = ids.to_vec();
        self.with_conn(move |conn| {
            // Một lần lấy khoá, một statement dùng lại cho cả lô. Index `events_by_type`
            // phủ đúng `(session_id, type, seq)` nên mỗi lượt là một lần tìm trong index,
            // không phải một lần quét.
            let mut stmt = conn.prepare_cached(
                "SELECT e.type, e.data
                   FROM events e
                   JOIN sessions s ON s.id = e.session_id
                  WHERE s.session_key = ?1
                    AND e.type IN ('user/message', 'assistant/message')
                  ORDER BY e.seq DESC
                  LIMIT 1",
            )?;

            let mut out = HashMap::new();
            for id in ids {
                let row: Option<(String, String)> = stmt
                    .query_row(params![id], |row| Ok((row.get(0)?, row.get(1)?)))
                    .optional()?;
                let Some((kind, data)) = row else { continue };
                if let Some(text) = preview_text(&kind, &data) {
                    out.insert(id, text);
                }
            }
            Ok(out)
        })
        .await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            // Chỉ xoá hàng phiên: `events.session_id` khai `ON DELETE CASCADE` và
            // `foreign_keys` bật, nên sự kiện đi theo. Xoá tay thêm một lần nữa là dựng
            // một bản sao thứ hai của cùng một luật, và hai bản sao thì trôi ra khỏi nhau.
            let touched =
                conn.execute("DELETE FROM sessions WHERE session_key = ?1", params![id])?;
            if touched == 0 {
                return Err(SessionError::NotFound(id));
            }
            Ok(())
        })
        .await
    }

    async fn set_title(&self, id: &str, title: &str) -> Result<()> {
        let id = id.to_owned();
        let title = title.to_owned();
        self.with_conn(move |conn| {
            let touched = conn.execute(
                "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE session_key = ?1",
                params![id, title, now_ms()],
            )?;
            if touched == 0 {
                return Err(SessionError::NotFound(id));
            }
            Ok(())
        })
        .await
    }
}
