//! Background commands.
//!
//! A backgrounded `bash` returns a `job_id` immediately and keeps running after the turn
//! has ended. That is what makes it useful (start `npm run dev`, then do something else)
//! and also what makes it dangerous: a process that outlives the thing that spawned it is
//! a process nobody remembers to clean up.
//!
//! So the job table's lifetime is tied to the plugin: disposing the plugin kills them all.
//! There is no path by which a job survives unloading, not even if someone forgets.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::provider::Execution;

#[derive(Clone, Debug)]
pub enum JobState {
    Running,
    Finished(Execution),
}

pub struct Job {
    pub id: String,
    pub command: String,
    pub cwd: String,
    pub cancel: CancellationToken,
    pub state: Mutex<JobState>,
}

#[derive(Default)]
pub struct Jobs {
    entries: Mutex<HashMap<String, Arc<Job>>>,
}

impl Jobs {
    pub fn start(&self, command: String, cwd: String, cancel: CancellationToken) -> Arc<Job> {
        let job = Arc::new(Job {
            id: uuid::Uuid::now_v7().to_string(),
            command,
            cwd,
            cancel,
            state: Mutex::new(JobState::Running),
        });
        self.entries.lock().insert(job.id.clone(), job.clone());
        job
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.entries.lock().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<Job>> {
        let mut all: Vec<_> = self.entries.lock().values().cloned().collect();
        all.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// Kill one job. Returns `false` when no job carries that id.
    pub fn kill(&self, id: &str) -> bool {
        match self.entries.lock().get(id) {
            Some(job) => {
                job.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Kill everything. Called when the plugin is disposed.
    pub fn kill_all(&self) {
        for job in self.entries.lock().values() {
            job.cancel.cancel();
        }
    }
}
