#[allow(unused_imports)]
use super::*;

pub struct ExecutionTaskResult {
    pub success: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub message: Option<String>,
}

impl ExecutionTaskResult {
    pub fn duration_ms(&self) -> i64 {
        (self.finished_at - self.started_at).num_milliseconds()
    }
}
