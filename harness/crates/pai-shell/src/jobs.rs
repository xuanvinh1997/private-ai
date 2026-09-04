//! Background commands: a backgrounded `bash` returns a `job_id` and outlives the turn.
//! The job table's lifetime is tied to the plugin, so disposing it kills every job; no job
//! can survive unloading.

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
