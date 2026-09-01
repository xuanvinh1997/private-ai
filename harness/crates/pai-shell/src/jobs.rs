//! Lệnh chạy nền.
//!
//! Một `bash` chạy nền trả `job_id` ngay rồi tiếp tục chạy sau khi lượt đã kết thúc. Đó
//! là thứ khiến nó hữu ích (chạy `npm run dev` rồi làm việc khác) và cũng là thứ khiến nó
//! nguy hiểm: một tiến trình sống lâu hơn thứ sinh ra nó là một tiến trình không ai còn
//! nhớ để dọn.
//!
//! Nên vòng đời của sổ job gắn với plugin: gỡ plugin là giết sạch. Không có đường nào để
//! một job sống sót qua lần gỡ tải, kể cả khi ai đó quên.

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

    /// Giết một job. Trả `false` nếu không có job nào mang id đó.
    pub fn kill(&self, id: &str) -> bool {
        match self.entries.lock().get(id) {
            Some(job) => {
                job.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Giết tất. Gọi khi plugin bị gỡ.
    pub fn kill_all(&self) {
        for job in self.entries.lock().values() {
            job.cancel.cancel();
        }
    }
}
