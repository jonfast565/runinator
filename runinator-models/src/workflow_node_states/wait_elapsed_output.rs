#[allow(unused_imports)]
use super::*;

/// wait node output recorded when the deadline elapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitElapsedOutput {
    pub deadline_unix: i64,
}
