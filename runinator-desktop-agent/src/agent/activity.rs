#[allow(unused_imports)]
use super::*;

/// A live dashboard activity and the instant at which it last materially changed. Repeated
/// heartbeats with the same status deliberately do not reset `since`, so the GUI can show how long
/// the agent has been waiting, reconnecting, or executing work.
#[derive(Debug, Clone)]
pub struct Activity {
    pub label: String,
    pub since: std::time::Instant,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            label: "not started".to_string(),
            since: std::time::Instant::now(),
        }
    }
}
