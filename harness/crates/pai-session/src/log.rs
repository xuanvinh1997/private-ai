//! Sổ trong bộ nhớ: mảng chỉ-ghi-thêm cộng phép chiếu ra lịch sử mô hình.
//!
//! Không có `history: Vec<Message>` sống song song ở đâu cả. Nếu có, sẽ tồn tại hai
//! nguồn sự thật, và cái thứ hai chắc chắn sẽ lệch — thường là vào lúc khó chẩn đoán
//! nhất, giữa một lần nén ngữ cảnh.

use std::sync::Mutex;

use crate::error::{Result, SessionError};
use crate::event::{Seq, SessionEvent, SessionEventEnvelope};
use crate::message::Message;
use crate::surface::{Surface, SurfaceOp};

/// Bộ nhớ đệm của [`SessionLog::derive_messages`].
///
/// Chi phí thường trực là O(node mới). Một lần `replace` làm `generation` nhảy và buộc
/// dựng lại toàn bộ — đó là cái giá đúng, vì replace đổi cả hình dạng lịch sử.
#[derive(Default)]
struct DeriveCache {
    generation: u64,
    /// Số node đã gấp, **không** phải số message: node rỗng không đẻ ra message nào.
    folded: usize,
    messages: Vec<Message>,
}

pub struct SessionLog {
    /// Chỉ số trong mảng chính là `seq`. Bất biến này được kiểm ở mọi lối vào.
    events: Vec<SessionEventEnvelope>,
    surface: Surface,
    cache: Mutex<DeriveCache>,
}

impl Default for SessionLog {
    fn default() -> Self {
        SessionLog::new()
    }
}

impl SessionLog {
    pub fn new() -> SessionLog {
        SessionLog {
            events: Vec::new(),
            surface: Surface::default(),
            cache: Mutex::new(DeriveCache::default()),
        }
    }

    /// Phát lại một sổ đã lưu. Đây là chỗ duy nhất kiểm được rằng thứ đọc lên vẫn liền
    /// mạch — kho lưu trữ có thể đã bị chép, cắt, hay ghi bởi hai tiến trình.
    pub fn replay(events: Vec<SessionEventEnvelope>) -> Result<SessionLog> {
        let mut log = SessionLog::new();
        for envelope in events {
            let expected = log.next_seq();
            if envelope.seq != expected {
                return Err(SessionError::SeqGap {
                    expected,
                    found: envelope.seq,
                });
            }
            log.push(envelope)?;
        }
        Ok(log)
    }

    pub fn next_seq(&self) -> Seq {
        self.events.len() as Seq
    }

    pub fn events(&self) -> &[SessionEventEnvelope] {
        &self.events
    }

    pub fn get(&self, seq: Seq) -> Option<&SessionEventEnvelope> {
        self.events.get(seq as usize)
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Ghi thêm một sự kiện log-only.
    pub fn append(&mut self, event: SessionEvent, time: i64) -> Result<Seq> {
        self.push_new(event, time, None, None)
    }

    /// Ghi thêm một sự kiện surface vào cuối lịch sử.
    pub fn append_surface(&mut self, event: SessionEvent, time: i64) -> Result<Seq> {
        self.push_new(event, time, Some(SurfaceOp::Append), None)
    }

    /// Ghi thêm một sự kiện surface che dải node `start..end`.
    ///
    /// Danh sách node bị che được tính **tại đây** chứ không do người gọi truyền vào: đó
    /// là cách duy nhất để "replace phải kê đủ" là một bất biến chứ không phải một lời
    /// dặn trong tài liệu.
    pub fn append_replacing(
        &mut self,
        event: SessionEvent,
        start: usize,
        end: usize,
        time: i64,
    ) -> Result<Seq> {
        let shadowed = self.surface.shadowed(start, end)?;
        self.push_new(
            event,
            time,
            Some(SurfaceOp::Replace { start, end }),
            Some(shadowed),
        )
    }

    fn push_new(
        &mut self,
        event: SessionEvent,
        time: i64,
        surface_op: Option<SurfaceOp>,
        source_event_seqs: Option<Vec<Seq>>,
    ) -> Result<Seq> {
        let seq = self.next_seq();
        let envelope = SessionEventEnvelope {
            seq,
            time,
            ignorable: None,
            event,
            source_event_seqs,
            surface_op,
        };
        self.push(envelope)?;
        Ok(seq)
    }

    fn push(&mut self, envelope: SessionEventEnvelope) -> Result<()> {
        envelope.check_surface_shape()?;
        if let Some(op) = envelope.surface_op {
            self.surface
                .apply(envelope.seq, op, envelope.source_event_seqs.as_deref())?;
        }
        self.events.push(envelope);
        Ok(())
    }

    /// Lịch sử mà mô hình thấy.
    ///
    /// Chỉ ba loại surface đi qua đây, nguyên văn. Một `assistant/message` rỗng nội dung
    /// bị loại khỏi lịch sử nhưng **vẫn nằm trong sổ** — nó là bằng chứng của một bước đã
    /// chạy và đã tiêu token.
    pub fn derive_messages(&self) -> Vec<Message> {
        let Ok(mut cache) = self.cache.lock() else {
            // Khoá nhiễm độc chỉ là mất phần đệm, không mất dữ liệu: dựng lại từ đầu.
            return self.fold_from(0);
        };
        if cache.generation != self.surface.generation() {
            cache.generation = self.surface.generation();
            cache.folded = 0;
            cache.messages.clear();
        }
        let nodes = self.surface.nodes();
        for seq in &nodes[cache.folded..] {
            if let Some(message) = self.get(*seq).and_then(SessionEventEnvelope::message) {
                cache.messages.push(message.clone());
            }
        }
        cache.folded = nodes.len();
        cache.messages.clone()
    }

    fn fold_from(&self, start: usize) -> Vec<Message> {
        self.surface.nodes()[start..]
            .iter()
            .filter_map(|seq| self.get(*seq).and_then(SessionEventEnvelope::message))
            .cloned()
            .collect()
    }

    /// Lượt đang mở tại `boundary`, nếu có.
    ///
    /// Tìm cặp `turn/start` / `turn/end` cuối cùng trong `[0..=boundary]`. Nếu cái cuối là
    /// `turn/start` thì tại điểm đó có một lượt chưa đóng.
    pub fn open_turn_at(&self, boundary: Seq) -> Option<u64> {
        if self.events.is_empty() {
            return None;
        }
        let upto = (boundary as usize).min(self.events.len() - 1);
        self.events[..=upto]
            .iter()
            .rev()
            .find_map(|e| match &e.event {
                SessionEvent::TurnStart(t) => Some(Some(t.turn)),
                SessionEvent::TurnEnd(_) => Some(None),
                _ => None,
            })?
    }
}
