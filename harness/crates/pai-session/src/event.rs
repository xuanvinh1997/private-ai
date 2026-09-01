//! Từ vựng sự kiện.
//!
//! Một bất biến chi phối cả tệp này: **cái gì mô hình thấy được thì phải nằm trong sổ.**
//! Hệ quả trực tiếp là thêm một loại đầu vào mới cho mô hình = thêm một loại sự kiện mới,
//! chứ không phải nhét thêm một trường vào chỗ nào đó ngoài sổ.
//!
//! Bộ v0.1 ở đây là mười loại. Bộ đầy đủ là năm mươi ba. Chênh lệch đó là lý do
//! [`SessionEvent`] có nhánh [`SessionEvent::Unknown`] và envelope có cờ `ignorable`:
//! một bản cũ đọc sổ do bản mới ghi phải **từ chối**, trừ khi bản mới đã tự nhận rằng
//! loại đó bỏ qua được. Im lặng lướt qua một loại lạ là cách êm ái nhất để dựng lại một
//! lịch sử thiếu mà không ai biết.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, SessionError};
use crate::message::Message;
use crate::surface::SurfaceOp;

pub type Seq = u64;

/// Nằm ở header của phiên, **không** nằm trong sổ: sổ chỉ-ghi-thêm không có chỗ cho một
/// con số thay đổi được.
pub const SESSION_FORMAT_VERSION: i64 = 1;

/// Đúng ba loại này sinh ra message cho mô hình. Mọi loại còn lại chỉ để ghi lại.
///
/// Danh sách này là ranh giới giữa "sổ" và "ngữ cảnh": nó ngắn có chủ ý, và mỗi lần nó
/// dài ra là một lần cả bộ nhớ của mô hình đổi hình dạng.
pub const SURFACE_TYPES: [&str; 3] = ["user/message", "assistant/message", "tool/result"];

// --- payload theo từng loại ----------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TurnStart {
    pub turn: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TurnEnd {
    pub turn: u64,
    pub reason: TurnEndReason,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TurnEndReason {
    Completed,
    /// Lý do duy nhất vòng lặp không bao giờ tự phát. Nó chỉ xuất hiện khi bộ khôi phục
    /// đóng một lượt mồ côi sau sự cố — nên thấy nó là biết đã có một lần chết giữa chừng.
    Interrupted,
    MaxSteps,
    Error {
        message: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StepStart {
    pub turn: u64,
    pub step: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StepEnd {
    pub turn: u64,
    pub step: u64,
}

/// Một mảnh stream thô.
///
/// `chunk` để nguyên `Value`: từ vựng stream thuộc về `pai-llm`, và sổ không cần hiểu nó
/// để làm đúng việc của mình. Cái sổ cần biết chỉ là `(turn, step)` — đủ để gói nhiều
/// mảnh liên tiếp vào một hàng lưu trữ.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantChunk {
    pub turn: u64,
    pub step: u64,
    pub chunk: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssistantMessage {
    pub turn: u64,
    pub step: u64,
    pub message: Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub turn: u64,
    pub step: u64,
    pub call_id: String,
    pub name: String,
    /// Chuỗi JSON thô mô hình sinh ra, giữ nguyên byte.
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub turn: u64,
    pub step: u64,
    pub call_id: String,
    pub message: Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolErrorInfo>,
    /// Dữ liệu cho giao diện (diff, đường dẫn spill…). Mô hình không thấy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ToolErrorInfo {
    pub name: String,
    pub code: String,
}

/// Phần đầu bất biến của một request: system prompt, schema tool, tham số mô hình.
///
/// Ghi lại nó là điều kiện để phát lại byte-for-byte về sau — cũng là thứ cho phép tận
/// dụng KV cache khi nén ngữ cảnh dựng lại đúng vùng bị che.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RequestHeader {
    pub header: Value,
    pub reason: RequestReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_series: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestReason {
    Initial,
    Resume,
    Change,
    Series,
}

/// Một loại do bản mới hơn ghi ra. Giữ nguyên văn để ghi lại được y như cũ.
#[derive(Clone, Debug, PartialEq)]
pub struct UnknownEvent {
    pub kind: String,
    pub data: Value,
}

// --- enum sự kiện ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    TurnStart(TurnStart),
    TurnEnd(TurnEnd),
    StepStart(StepStart),
    StepEnd(StepEnd),
    UserMessage(Message),
    AssistantChunk(AssistantChunk),
    AssistantMessage(AssistantMessage),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    RequestHeader(RequestHeader),
    Unknown(UnknownEvent),
}

impl SessionEvent {
    pub fn type_name(&self) -> &str {
        match self {
            SessionEvent::TurnStart(_) => "turn/start",
            SessionEvent::TurnEnd(_) => "turn/end",
            SessionEvent::StepStart(_) => "step/start",
            SessionEvent::StepEnd(_) => "step/end",
            SessionEvent::UserMessage(_) => "user/message",
            SessionEvent::AssistantChunk(_) => "assistant/chunk",
            SessionEvent::AssistantMessage(_) => "assistant/message",
            SessionEvent::ToolCall(_) => "tool/call",
            SessionEvent::ToolResult(_) => "tool/result",
            SessionEvent::RequestHeader(_) => "request/header",
            SessionEvent::Unknown(u) => &u.kind,
        }
    }

    /// Loại này có sinh message cho mô hình không?
    pub fn is_surface(&self) -> bool {
        matches!(
            self,
            SessionEvent::UserMessage(_)
                | SessionEvent::AssistantMessage(_)
                | SessionEvent::ToolResult(_)
        )
    }

    /// Payload dạng JSON — chính là thứ nằm ở cột `data`.
    pub fn data(&self) -> Result<Value> {
        let value = match self {
            SessionEvent::TurnStart(p) => serde_json::to_value(p)?,
            SessionEvent::TurnEnd(p) => serde_json::to_value(p)?,
            SessionEvent::StepStart(p) => serde_json::to_value(p)?,
            SessionEvent::StepEnd(p) => serde_json::to_value(p)?,
            SessionEvent::UserMessage(p) => serde_json::to_value(p)?,
            SessionEvent::AssistantChunk(p) => serde_json::to_value(p)?,
            SessionEvent::AssistantMessage(p) => serde_json::to_value(p)?,
            SessionEvent::ToolCall(p) => serde_json::to_value(p)?,
            SessionEvent::ToolResult(p) => serde_json::to_value(p)?,
            SessionEvent::RequestHeader(p) => serde_json::to_value(p)?,
            SessionEvent::Unknown(u) => u.data.clone(),
        };
        Ok(value)
    }

    /// Dựng lại từ hai cột `(type, data)` — đường đọc chính, không đi qua envelope JSON.
    ///
    /// Loại lạ **không** là lỗi ở đây. Quyết định nhận hay từ chối cần thêm cờ
    /// `ignorable`, mà cờ đó nằm ở envelope; xem [`SessionEventEnvelope::from_parts`].
    pub fn from_parts(kind: &str, data: Value) -> Result<SessionEvent> {
        let event = match kind {
            "turn/start" => SessionEvent::TurnStart(serde_json::from_value(data)?),
            "turn/end" => SessionEvent::TurnEnd(serde_json::from_value(data)?),
            "step/start" => SessionEvent::StepStart(serde_json::from_value(data)?),
            "step/end" => SessionEvent::StepEnd(serde_json::from_value(data)?),
            "user/message" => SessionEvent::UserMessage(serde_json::from_value(data)?),
            "assistant/chunk" => SessionEvent::AssistantChunk(serde_json::from_value(data)?),
            "assistant/message" => SessionEvent::AssistantMessage(serde_json::from_value(data)?),
            "tool/call" => SessionEvent::ToolCall(serde_json::from_value(data)?),
            "tool/result" => SessionEvent::ToolResult(serde_json::from_value(data)?),
            "request/header" => SessionEvent::RequestHeader(serde_json::from_value(data)?),
            other => SessionEvent::Unknown(UnknownEvent {
                kind: other.to_owned(),
                data,
            }),
        };
        Ok(event)
    }

    /// Message mà loại này chiếu ra, nếu có.
    ///
    /// Nguyên văn, không thêm khung. Một `assistant/message` rỗng nội dung trả `None`:
    /// nó tồn tại chỉ để giữ `usage` của một bước bị cụt vì hết token, và đẩy một message
    /// rỗng vào lịch sử là làm nhiều nhà cung cấp mô hình từ chối cả request.
    pub fn message(&self) -> Option<&Message> {
        match self {
            SessionEvent::UserMessage(m) => Some(m),
            SessionEvent::AssistantMessage(a) if !a.message.is_empty() => Some(&a.message),
            SessionEvent::ToolResult(t) => Some(&t.message),
            _ => None,
        }
    }
}

/// Hai cột `{type, data}` — hình dạng trên dây và trong DB.
#[derive(Serialize, Deserialize)]
struct Tagged {
    #[serde(rename = "type")]
    kind: String,
    data: Value,
}

impl Serialize for SessionEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("type", self.type_name())?;
        match self {
            SessionEvent::TurnStart(p) => map.serialize_entry("data", p)?,
            SessionEvent::TurnEnd(p) => map.serialize_entry("data", p)?,
            SessionEvent::StepStart(p) => map.serialize_entry("data", p)?,
            SessionEvent::StepEnd(p) => map.serialize_entry("data", p)?,
            SessionEvent::UserMessage(p) => map.serialize_entry("data", p)?,
            SessionEvent::AssistantChunk(p) => map.serialize_entry("data", p)?,
            SessionEvent::AssistantMessage(p) => map.serialize_entry("data", p)?,
            SessionEvent::ToolCall(p) => map.serialize_entry("data", p)?,
            SessionEvent::ToolResult(p) => map.serialize_entry("data", p)?,
            SessionEvent::RequestHeader(p) => map.serialize_entry("data", p)?,
            SessionEvent::Unknown(u) => map.serialize_entry("data", &u.data)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<SessionEvent, D::Error> {
        let tagged = Tagged::deserialize(d)?;
        SessionEvent::from_parts(&tagged.kind, tagged.data).map_err(de::Error::custom)
    }
}

// --- envelope ---------------------------------------------------------------------------

/// Vỏ bọc quanh mỗi sự kiện.
///
/// `seq` liền mạch kể cả với mảnh stream thô. Đó là điều kiện để kho lưu trữ chép lại
/// nguyên văn cả sổ mà không phải đánh số lại — và để bất kỳ ai cũng phát hiện được một
/// lỗ hổng chỉ bằng phép so sánh chỉ số.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionEventEnvelope {
    pub seq: Seq,
    /// Epoch ms.
    pub time: i64,
    #[serde(flatten)]
    pub event: SessionEvent,
    /// Vắng nghĩa là **bắt buộc**: reader không hiểu loại này thì phải từ chối cả sổ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignorable: Option<bool>,
    /// Những seq đã đẻ ra sự kiện này. Với `replace`, đây là toàn bộ node bị che.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_seqs: Option<Vec<Seq>>,
    /// Bắt buộc trên sự kiện surface, và cấm trên mọi loại khác.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_op: Option<SurfaceOp>,
}

impl SessionEventEnvelope {
    /// Dựng lại từ các cột đã lưu, và thi hành hai luật đọc.
    ///
    /// Luật thứ nhất: loại lạ mà không tự nhận bỏ qua được thì từ chối cả sổ.
    /// Luật thứ hai: `surface_op` chỉ được có mặt trên đúng ba loại surface — nếu không,
    /// phép chiếu sẽ nhìn thấy một lịch sử khác với thứ đã thật sự gửi cho mô hình.
    pub fn from_parts(
        seq: Seq,
        time: i64,
        kind: &str,
        data: Value,
        ignorable: Option<bool>,
        source_event_seqs: Option<Vec<Seq>>,
        surface_op: Option<SurfaceOp>,
    ) -> Result<SessionEventEnvelope> {
        let event = SessionEvent::from_parts(kind, data)?;
        if matches!(event, SessionEvent::Unknown(_)) && ignorable != Some(true) {
            return Err(SessionError::FormatUnsupported(kind.to_owned()));
        }
        let envelope = SessionEventEnvelope {
            seq,
            time,
            event,
            ignorable,
            source_event_seqs,
            surface_op,
        };
        envelope.check_surface_shape()?;
        Ok(envelope)
    }

    pub(crate) fn check_surface_shape(&self) -> Result<()> {
        let name = self.event.type_name();
        match (self.event.is_surface(), self.surface_op.is_some()) {
            (true, false) => Err(SessionError::SurfaceOpRequired(surface_name(name))),
            (false, true) => Err(SessionError::SurfaceOpForbidden(surface_name(name))),
            _ => Ok(()),
        }
    }

    pub fn message(&self) -> Option<&Message> {
        self.event.message()
    }
}

/// `SessionError` cầm `&'static str`; danh sách loại surface là hằng, còn loại lạ thì
/// gộp về một nhãn chung thay vì rò rỉ một chuỗi có vòng đời ngắn hơn.
fn surface_name(name: &str) -> &'static str {
    SURFACE_TYPES
        .iter()
        .copied()
        .find(|known| *known == name)
        .unwrap_or("<loại khác>")
}
