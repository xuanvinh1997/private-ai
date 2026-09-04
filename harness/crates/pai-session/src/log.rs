//! In-memory log: an append-only vector plus the projection to model history.
//! No parallel `history: Vec<Message>` anywhere -- a second source of truth would drift,
//! usually mid-compaction where it is hardest to diagnose.

use std::sync::Mutex;

use crate::error::{Result, SessionError};
use crate::event::{Seq, SessionEvent, SessionEventEnvelope};
use crate::message::Message;
use crate::surface::{Surface, SurfaceOp};

/// Cache for [`SessionLog::derive_messages`]: O(new nodes) normally, full rebuild when a `replace` bumps `generation`.
#[derive(Default)]
struct DeriveCache {
    generation: u64,
    /// Nodes folded so far, not messages: an empty node produces none.
    folded: usize,
    messages: Vec<Message>,
}

pub struct SessionLog {
    /// The index in this vector is the `seq`; the invariant is checked at every entry point.
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

    /// Replay a stored log; the only place that can verify it is still gapless after copies, truncation or two writers.
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

    /// Append a log-only event.
    pub fn append(&mut self, event: SessionEvent, time: i64) -> Result<Seq> {
        self.push_new(event, time, None, None)
    }

    /// Append a surface event at the end of history.
    pub fn append_surface(&mut self, event: SessionEvent, time: i64) -> Result<Seq> {
        self.push_new(event, time, Some(SurfaceOp::Append), None)
    }

    /// Append a surface event shadowing nodes `start..end`; the shadowed list is computed here, not passed in,
    /// so "replace must cite everything" is an invariant rather than documentation.
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

    /// The history the model sees: only the three surface types, verbatim. An empty `assistant/message`
    /// is dropped from history but stays in the log as evidence of a step that ran and spent tokens.
    pub fn derive_messages(&self) -> Vec<Message> {
        let Ok(mut cache) = self.cache.lock() else {
            // A poisoned lock loses only the cache, not data: rebuild from scratch.
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

    /// The turn open at `boundary`, if any: the last `turn/start` / `turn/end` in `[0..=boundary]` being a start.
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
