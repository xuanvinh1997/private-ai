//! Per-agent scoping.
//!
//! An unscoped registration is visible to every agent. A scoped one is visible only to
//! that agent — and its descendants. Events flow **up** the tree: a listener attached to a
//! parent agent receives events emitted by a child agent, never the other way around.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

/// The identity of one scope. Usually an agent or a session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ScopeKey(u64);

/// Parent–child relationships between scopes.
#[derive(Default)]
pub struct ScopeTree {
    parents: RwLock<HashMap<ScopeKey, Option<ScopeKey>>>,
}

impl ScopeTree {
    pub fn create(&self, parent: Option<ScopeKey>) -> ScopeKey {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let key = ScopeKey(NEXT.fetch_add(1, Ordering::Relaxed));
        self.parents.write().insert(key, parent);
        key
    }

    pub fn remove(&self, key: ScopeKey) {
        self.parents.write().remove(&key);
    }

    pub fn parent(&self, key: ScopeKey) -> Option<ScopeKey> {
        self.parents.read().get(&key).copied().flatten()
    }

    /// Does a listener tagged `tag` receive an event emitted at `dispatch`?
    ///
    /// No tag receives everything. A tag receives when it *is* the emitting scope, or is
    /// an ancestor of it.
    pub fn admits(&self, dispatch: Option<ScopeKey>, tag: Option<ScopeKey>) -> bool {
        let Some(tag) = tag else { return true };
        let mut cursor = dispatch;
        while let Some(key) = cursor {
            if key == tag {
                return true;
            }
            cursor = self.parent(key);
        }
        false
    }
}
