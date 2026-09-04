//! An open session, and what can be done with one.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Result, SessionError};
use crate::event::{Seq, SessionEvent, SessionEventEnvelope, TurnEnd, TurnEndReason};
use crate::log::SessionLog;
use crate::message::Message;
use crate::sqlite::now_ms;
use crate::store::{NewSession, SessionHeader, SessionId, SessionStore};

/// How many stream chunks may wait in memory before a forced write; this window is everything a crash
/// mid-answer can lose, and a hundred token-sized chunks is under a line of text.
const PENDING_LIMIT: usize = 100;

pub struct Session {
    header: SessionHeader,
    log: SessionLog,
    store: Arc<dyn SessionStore>,
    /// Events already in the in-memory log but not yet on disk.
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

    /// The history the model sees, projected from the log.
    pub fn derive_messages(&self) -> Vec<Message> {
        self.log.derive_messages()
    }

    /// Append a log-only event; surface events use a separate entry point so none can land without a `surface_op`.
    pub async fn append(&mut self, event: SessionEvent) -> Result<Seq> {
        let seq = self.log.append(event, now_ms())?;
        self.stage(seq).await?;
        Ok(seq)
    }

    /// Append a surface event at the end of history.
    pub async fn append_surface(&mut self, event: SessionEvent) -> Result<Seq> {
        let seq = self.log.append_surface(event, now_ms())?;
        self.stage(seq).await?;
        Ok(seq)
    }

    /// Append a surface event shadowing nodes `start..end` (positions, not seqs); nothing is deleted, only hidden from the projection.
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

    /// Flush pending events in one transaction; a failed write keeps the batch, since losing it would leave a `seq` gap.
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

    /// Set the title; titles live in metadata, not the log, because they are mutable and the log is not.
    pub async fn set_title(&mut self, title: &str) -> Result<()> {
        self.store.set_title(&self.header.id, title).await?;
        self.header.title = Some(title.to_owned());
        Ok(())
    }

    /// Close a turn orphaned by a crash by appending `turn/end` with `interrupted`; the log is never truncated.
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

/// The entry point for every session operation.
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

    /// Reopen a session: reload the whole log and rebuild the projection from scratch.
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

    /// Subtitle line for the session list. See [`SessionStore::previews`].
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
                // The durable lineage boundary, unlike "how much was replayed this run", which is runtime-only.
                seed_length: Some(boundary + 1),
                origin: parent.origin,
                delegation_depth: parent.delegation_depth,
                agent_preset: parent.agent_preset.clone(),
            })
            .await?;
        // The seed keeps its `seq` and `time`: the child must replay exactly what the parent sent, continuing at `boundary + 1`.
        self.store.append(&header.id, seed.clone()).await?;
        Ok(Session {
            header,
            log: SessionLog::replay(seed)?,
            store: self.store.clone(),
            pending: Vec::new(),
        })
    }
}
