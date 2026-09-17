#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeComponentSnapshot {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub restarts: u32,
    pub uptime_seconds: Option<u64>,
    pub last_error: Option<String>,
}
