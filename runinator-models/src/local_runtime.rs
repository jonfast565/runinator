//! local composition-host status shared by the API, standalone daemon, and command center.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalRuntimeHostKind {
    Standalone,
    Supervisor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeSnapshot {
    pub host_kind: LocalRuntimeHostKind,
    pub pid: u32,
    pub started_at: String,
    pub updated_at: String,
    pub components: Vec<LocalRuntimeComponentSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeComponentSnapshot {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub restarts: u32,
    pub uptime_seconds: Option<u64>,
    pub last_error: Option<String>,
}
