#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspacePhaseEvent {
    pub version: u8,
    pub phase: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub details: Value,
}
