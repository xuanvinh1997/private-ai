//! The loop's four hook points.
//! The loop knows nothing of approval, sandbox, hooks or compaction; every policy arrives
//! from outside through these four, which is why adding a feature never edits the loop.

use pai_core::{First, Waterfall};
use pai_llm::ChatRequest;
use pai_session::Message;

/// Mask a range of old nodes behind a summary; not a delete, since the journal keeps them replayable.
#[derive(Debug, Clone)]
pub struct Replacement {
    /// Node range, half-open.
    pub start: usize,
    pub end: usize,
    pub summary: Message,
}

/// The `agent/pre-step` decision.
#[derive(Debug, Clone)]
pub enum StepDecision {
    /// Enter the step with exactly these messages; a listener may edit the list.
    Enter {
        messages: Vec<Message>,
        /// Mask history before building the request; indices refer to the projection the listener just saw.
        replace: Option<Replacement>,
    },
    /// Do not enter; the turn is still journalled and closed, because the record must remember the attempt.
    Reject { reason: String },
}

impl StepDecision {
    /// Enter with no masking. The common case.
    pub fn enter(messages: Vec<Message>) -> StepDecision {
        StepDecision::Enter {
            messages,
            replace: None,
        }
    }
}

pub struct PreStepRequest {
    pub turn: u64,
    pub step: u64,
    pub messages: Vec<Message>,
    /// The journal's current projection; compaction has to measure exactly what the model will see.
    pub history: Vec<Message>,
}

/// What enters a step; the compaction policy hooks in here.
pub enum PreStep {}
impl Waterfall for PreStep {
    const NAME: &'static str = "agent/pre-step";
    type Req = PreStepRequest;
    type Out = StepDecision;
}

/// The final request to the model: where context is added, models swapped, history trimmed.
pub enum AgentRequest {}
impl Waterfall for AgentRequest {
    const NAME: &'static str = "agent/request";
    type Req = ChatRequest;
    type Out = ChatRequest;
}

pub struct TurnStopping {
    pub turn: u64,
}

/// The last gate before a turn closes; returning `Some` keeps it open for one more step.
pub enum TurnStop {}
impl First for TurnStop {
    const NAME: &'static str = "agent/turn-stopping";
    type Payload = TurnStopping;
    type Out = Vec<Message>;
}
