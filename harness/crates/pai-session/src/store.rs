//! The persistence seam and the titling seam, following `pai-core`'s pattern: an uninhabited marker
//! key plus a trait object, so swapping SQLite for JSONL or a remote store changes one provider line.

use async_trait::async_trait;
use pai_core::ServiceKey;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::event::{SESSION_FORMAT_VERSION, Seq, SessionEventEnvelope};
use crate::log::SessionLog;

/// A session's public identity; UUID v7 so ids sort by creation time.
pub type SessionId = String;

pub fn new_session_id() -> SessionId {
    uuid::Uuid::now_v7().to_string()
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Subagent,
}

impl Origin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Origin::Subagent => "subagent",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Origin> {
        match raw {
            "subagent" => Some(Origin::Subagent),
            _ => None,
        }
    }
}

/// Session metadata, deliberately outside the log: an append-only log has no room for mutable fields like the title.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionHeader {
    pub id: SessionId,
    pub format_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub title: Option<String>,
    /// Absolute and canonicalized. `None` means the session is not bound to a directory.
    pub cwd: Option<String>,
    pub parent_session: Option<SessionId>,
    /// How many leading events were inherited from the parent: the durable fork boundary, not a runtime replay count.
    pub seed_length: Option<u64>,
    pub origin: Option<Origin>,
    pub delegation_depth: Option<u32>,
    pub agent_preset: Option<String>,
}

/// Session creation request; an empty `id` lets the store generate one.
#[derive(Clone, Debug, Default)]
pub struct NewSession {
    pub id: Option<SessionId>,
    pub cwd: Option<String>,
    pub parent_session: Option<SessionId>,
    pub seed_length: Option<u64>,
    pub origin: Option<Origin>,
    pub delegation_depth: Option<u32>,
    pub agent_preset: Option<String>,
}

impl NewSession {
    pub fn in_dir(cwd: impl Into<String>) -> NewSession {
        NewSession {
            cwd: Some(cwd.into()),
            ..NewSession::default()
        }
    }

    pub(crate) fn format_version(&self) -> i64 {
        SESSION_FORMAT_VERSION
    }
}

/// Which sessions a listing is asking for.
///
/// A session records the directory it was opened in, so "the sessions of this project" is a real
/// question the store can answer. Asking it here rather than filtering the answer afterwards is not
/// tidiness: `list` takes a limit, and a limit applied before the filter silently hides a project's
/// older sessions behind another project's newer ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionScope<'a> {
    /// Every session, whatever directory it belongs to.
    All,
    /// Sessions opened in this directory.
    Directory(&'a str),
    /// Sessions bound to no directory - what plain conversation with no project open produces.
    Unbound,
}

/// The session store. `append` takes a batch, not one event: chunks arrive densely and a transaction per chunk makes the disk the bottleneck.
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn create(&self, spec: NewSession) -> Result<SessionHeader>;

    /// Newest first.
    async fn list(&self, scope: SessionScope<'_>, limit: Option<u32>)
    -> Result<Vec<SessionHeader>>;

    async fn header(&self, id: &str) -> Result<SessionHeader>;

    /// The batch must start exactly at the session's `last_seq + 1`; this is the last defence of the gapless-seq invariant.
    async fn append(&self, id: &str, events: Vec<SessionEventEnvelope>) -> Result<()>;

    /// Read everything back in seq order.
    async fn load(&self, id: &str) -> Result<Vec<SessionEventEnvelope>>;

    /// Actual rows in the table, which differs from the event count because chunks share rows; used for metrics and tests.
    async fn row_count(&self, id: &str) -> Result<u64>;

    async fn set_title(&self, id: &str, title: &str) -> Result<()>;

    /// Last line said in each session, for list subtitles; batched because a per-session async loop would
    /// contend on the store lock once per session. Sessions with nothing said are simply absent.
    async fn previews(&self, ids: &[String]) -> Result<HashMap<String, String>>;

    /// Delete a session and all its events; append-only governs editing history within a session, not discarding one.
    async fn delete(&self, id: &str) -> Result<()>;
}

/// The session-store seam.
pub enum Sessions {}

impl ServiceKey for Sessions {
    type Api = dyn SessionStore;
    const NAME: &'static str = "sessions";
}

/// The session-titling seam, separate from the store because titling is policy: ask the model, take the first line, or let the user type it.
#[async_trait]
pub trait SessionTitler: Send + Sync + 'static {
    /// `None` means not enough to go on yet -- a valid answer, not an error.
    async fn title(&self, log: &SessionLog) -> Result<Option<String>>;
}

pub enum SessionTitle {}

impl ServiceKey for SessionTitle {
    type Api = dyn SessionTitler;
    const NAME: &'static str = "session.title";
}

/// v0.1's only provider: no titling at all, present so the seam exists and consumers code against `Option<String>` once.
pub struct NoTitle;

#[async_trait]
impl SessionTitler for NoTitle {
    async fn title(&self, _log: &SessionLog) -> Result<Option<String>> {
        Ok(None)
    }
}

/// Fork boundary; the `seq` is inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boundary(pub Seq);
