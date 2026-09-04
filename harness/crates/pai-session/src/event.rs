//! Event vocabulary. Anything the model sees must be in the log, so a new kind of model input is a
//! new event type. v0.1 defines ten of an eventual fifty-three, hence [`SessionEvent::Unknown`] and
//! the `ignorable` flag: an older build must reject a newer type unless it was declared skippable.

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, SessionError};
use crate::message::Message;
use crate::surface::SurfaceOp;

pub type Seq = u64;

/// Lives in the session header, not the log: an append-only log has no room for a mutable number.
pub const SESSION_FORMAT_VERSION: i64 = 1;

/// Exactly these three produce messages for the model; every other type is record-only. The list is
/// the boundary between log and context, and each addition reshapes the model's memory.
pub const SURFACE_TYPES: [&str; 3] = ["user/message", "assistant/message", "tool/result"];

// --- per-type payloads -----------------------------------------------------------------

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
    /// The one reason the loop never emits itself: recovery closes an orphaned turn, so it marks a crash.
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

/// A raw stream chunk. `chunk` stays a `Value` because stream vocabulary belongs to `pai-llm`;
/// the log only needs `(turn, step)` to pack consecutive chunks into one stored row.
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
    /// Raw JSON string as the model produced it, byte for byte.
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
    /// UI-only data (diffs, spill paths); never shown to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ToolErrorInfo {
    pub name: String,
    pub code: String,
}

/// The invariant head of a request (system prompt, tool schemas, model params); recording it is what
/// makes byte-for-byte replay -- and KV-cache reuse after compaction -- possible.
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

/// A type written by a newer build, kept verbatim so it can be written back unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct UnknownEvent {
    pub kind: String,
    pub data: Value,
}

// --- event enum ------------------------------------------------------------------------

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

    /// Does this type produce a message for the model?
    pub fn is_surface(&self) -> bool {
        matches!(
            self,
            SessionEvent::UserMessage(_)
                | SessionEvent::AssistantMessage(_)
                | SessionEvent::ToolResult(_)
        )
    }

    /// The JSON payload, exactly what sits in the `data` column.
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

    /// Rebuild from the `(type, data)` columns; an unknown type is not an error here, since deciding
    /// needs the envelope's `ignorable` flag -- see [`SessionEventEnvelope::from_parts`].
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

    /// The message this type projects, verbatim. An empty `assistant/message` returns `None`: it exists
    /// only to carry `usage` from a truncated step, and empty messages make many providers reject the request.
    pub fn message(&self) -> Option<&Message> {
        match self {
            SessionEvent::UserMessage(m) => Some(m),
            SessionEvent::AssistantMessage(a) if !a.message.is_empty() => Some(&a.message),
            SessionEvent::ToolResult(t) => Some(&t.message),
            _ => None,
        }
    }
}

/// The `{type, data}` pair: the shape on the wire and in the database.
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

/// Envelope around each event. `seq` is gapless even for raw chunks, so a store can copy a log
/// verbatim without renumbering and anyone can spot a gap by comparing indices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionEventEnvelope {
    pub seq: Seq,
    /// Epoch ms.
    pub time: i64,
    #[serde(flatten)]
    pub event: SessionEvent,
    /// Absent means mandatory: a reader that does not understand the type must reject the whole log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignorable: Option<bool>,
    /// The seqs that produced this event; for `replace`, every shadowed node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_seqs: Option<Vec<Seq>>,
    /// Required on surface events, forbidden on every other type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_op: Option<SurfaceOp>,
}

impl SessionEventEnvelope {
    /// Rebuild from stored columns and enforce two read rules: an unknown type that is not skippable
    /// rejects the log, and `surface_op` may appear only on the three surface types.
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

/// `SessionError` holds a `&'static str`, so unknown types collapse to one label rather than leaking a short-lived string.
fn surface_name(name: &str) -> &'static str {
    SURFACE_TYPES
        .iter()
        .copied()
        .find(|known| *known == name)
        .unwrap_or("<loại khác>")
}
