//! Một phiên đang mở, và những việc làm được với một phiên.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Result, SessionError};
use crate::event::{Seq, SessionEvent, SessionEventEnvelope, TurnEnd, TurnEndReason};
use crate::log::SessionLog;
use crate::message::Message;
use crate::sqlite::now_ms;
use crate::store::{NewSession, SessionHeader, SessionId, SessionStore};

/// Bao nhiêu mảnh stream được phép nằm chờ trong bộ nhớ trước khi bắt buộc ghi xuống.
///
/// Cửa sổ này là toàn bộ phần dữ liệu có thể mất khi tiến trình chết giữa một câu trả lời.
/// Một trăm mảnh cỡ một token là chưa tới một dòng chữ — đủ nhỏ để không tiếc, đủ lớn để
/// việc gõ chữ không phải chờ ổ đĩa.
const PENDING_LIMIT: usize = 100;

pub struct Session {
    header: SessionHeader,
    log: SessionLog,
    store: Arc<dyn SessionStore>,
    /// Sự kiện đã vào sổ trong bộ nhớ nhưng chưa xuống đĩa.
    pending: Vec<SessionEventEnvelope>,
}

impl Session {
    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    pub fn id(&self) -> &str {
        &self.header.id
    }

    pub fn log(&self) -> &SessionLog {
        &self.log
    }

    /// Lịch sử mà mô hình thấy, chiếu ra từ sổ.
    pub fn derive_messages(&self) -> Vec<Message> {
        self.log.derive_messages()
    }

    /// Ghi thêm một sự kiện log-only.
    ///
    /// Sự kiện surface đi cửa khác. Tách hai cửa để một `user/message` không thể lọt vào
    /// sổ mà quên `surface_op` — cái sai đó im lặng, và nó làm mô hình mất một message.
    pub async fn append(&mut self, event: SessionEvent) -> Result<Seq> {
        let seq = self.log.append(event, now_ms())?;
        self.stage(seq).await?;
        Ok(seq)
    }

    /// Ghi thêm một sự kiện surface vào cuối lịch sử.
    pub async fn append_surface(&mut self, event: SessionEvent) -> Result<Seq> {
        let seq = self.log.append_surface(event, now_ms())?;
        self.stage(seq).await?;
        Ok(seq)
    }

    /// Ghi thêm một sự kiện surface che dải node `start..end` (vị trí, không phải seq).
    ///
    /// Không xoá gì cả: dải cũ vẫn nằm nguyên trong sổ và vẫn phát lại được. Chỉ phép
    /// chiếu ngừng nhìn thấy nó.
    pub async fn append_replacing(
        &mut self,
        event: SessionEvent,
        start: usize,
        end: usize,
    ) -> Result<Seq> {
        let seq = self.log.append_replacing(event, start, end, now_ms())?;
        self.stage(seq).await?;
        Ok(seq)
    }

    async fn stage(&mut self, seq: Seq) -> Result<()> {
        let envelope = self
            .log
            .get(seq)
            .cloned()
            .ok_or_else(|| SessionError::Unavailable(format!("sổ mất sự kiện {seq}")))?;
        let dense = matches!(envelope.event, SessionEvent::AssistantChunk(_));
        self.pending.push(envelope);
        if !dense || self.pending.len() >= PENDING_LIMIT {
            self.flush().await?;
        }
        Ok(())
    }

    /// Đẩy phần đang chờ xuống đĩa trong một giao dịch.
    ///
    /// Ghi hỏng thì lô được **giữ lại** chứ không bị vứt: mất một lô là thủng một lỗ trong
    /// `seq`, mà một lỗ thì không vá được bằng lần ghi sau.
    pub async fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        match self.store.append(&self.header.id, batch.clone()).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.pending = batch;
                Err(err)
            }
        }
    }

    /// Đặt tiêu đề. Tiêu đề nằm ở metadata, không nằm trong sổ — nó đổi được, còn sổ thì
    /// không.
    pub async fn set_title(&mut self, title: &str) -> Result<()> {
        self.store.set_title(&self.header.id, title).await?;
        self.header.title = Some(title.to_owned());
        Ok(())
    }

    /// Đóng một lượt mồ côi sau sự cố.
    ///
    /// Không cắt cụt sổ: sự kiện đã ghi là đã xảy ra. Thay vào đó ghi thêm một `turn/end`
    /// với lý do `interrupted` — lý do duy nhất vòng lặp không bao giờ tự phát, nên thấy
    /// nó là biết chắc đã có một lần chết giữa chừng.
    async fn heal_open_turn(&mut self) -> Result<()> {
        if self.log.is_empty() {
            return Ok(());
        }
        let last = self.log.next_seq() - 1;
        let Some(turn) = self.log.open_turn_at(last) else {
            return Ok(());
        };
        self.append(SessionEvent::TurnEnd(TurnEnd {
            turn,
            reason: TurnEndReason::Interrupted,
        }))
        .await?;
        Ok(())
    }
}

/// Cửa vào của mọi thao tác trên phiên.
#[derive(Clone)]
pub struct SessionService {
    store: Arc<dyn SessionStore>,
}

impl SessionService {
    pub fn new(store: Arc<dyn SessionStore>) -> SessionService {
        SessionService { store }
    }

    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    pub async fn create(&self, spec: NewSession) -> Result<Session> {
        let header = self.store.create(spec).await?;
        Ok(Session {
            header,
            log: SessionLog::new(),
            store: self.store.clone(),
            pending: Vec::new(),
        })
    }

    pub async fn list(&self, limit: Option<u32>) -> Result<Vec<SessionHeader>> {
        self.store.list(limit).await
    }

    /// Mở lại một phiên: đọc lại toàn bộ sổ và dựng lại phép chiếu từ đầu.
    pub async fn open(&self, id: &str) -> Result<Session> {
        let header = self.store.header(id).await?;
        let log = SessionLog::replay(self.store.load(id).await?)?;
        let mut session = Session {
            header,
            log,
            store: self.store.clone(),
            pending: Vec::new(),
        };
        session.heal_open_turn().await?;
        Ok(session)
    }

    /// Tách một phiên con mang `[0..=boundary]` làm hạt giống.
    ///
    /// `boundary` để trống là sự kiện cuối. Ranh giới sai **không** được làm tròn: một
    /// ranh giới sai là một ý định sai, và làm tròn nó đẻ ra một phiên con không ai yêu cầu.
    /// Dòng phụ cho danh sách phiên. Xem [`SessionStore::previews`].
    pub async fn previews(&self, ids: &[String]) -> Result<HashMap<String, String>> {
        self.store.previews(ids).await
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        self.store.delete(id).await
    }

    pub async fn rename(&self, id: &str, title: &str) -> Result<()> {
        self.store.set_title(id, title).await
    }

    pub async fn fork(&self, source: &str, boundary: Option<Seq>) -> Result<Session> {
        let parent = self.store.header(source).await?;
        let log = SessionLog::replay(self.store.load(source).await?)?;
        if log.is_empty() {
            return Err(SessionError::InvalidBoundary {
                boundary: boundary.unwrap_or(0),
                reason: "phiên nguồn chưa có sự kiện nào",
            });
        }
        let last = log.next_seq() - 1;
        let boundary = boundary.unwrap_or(last);
        if boundary > last {
            return Err(SessionError::InvalidBoundary {
                boundary,
                reason: "vượt quá sự kiện cuối của phiên nguồn",
            });
        }
        if let Some(turn) = log.open_turn_at(boundary) {
            return Err(SessionError::OpenTurn { boundary, turn });
        }

        let seed = log.events()[..=boundary as usize].to_vec();
        let header = self
            .store
            .create(NewSession {
                id: None,
                cwd: parent.cwd.clone(),
                parent_session: Some(SessionId::from(source)),
                // Ranh giới lineage, bền vững. Khác với "đã phát lại bao nhiêu trong vòng
                // đời này", vốn chỉ là chuyện lúc chạy.
                seed_length: Some(boundary + 1),
                origin: parent.origin,
                delegation_depth: parent.delegation_depth,
                agent_preset: parent.agent_preset.clone(),
            })
            .await?;
        // Hạt giống giữ nguyên `seq` và `time`: phiên con phải phát lại ra đúng thứ phiên
        // cha đã gửi cho mô hình, và sự kiện mới nối tiếp từ `boundary + 1`.
        self.store.append(&header.id, seed.clone()).await?;
        Ok(Session {
            header,
            log: SessionLog::replay(seed)?,
            store: self.store.clone(),
            pending: Vec::new(),
        })
    }
}
