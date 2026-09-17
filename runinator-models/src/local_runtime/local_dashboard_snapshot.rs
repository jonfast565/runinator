#[allow(unused_imports)]
use super::*;

/// Versioned, atomically-written local attachment data kept beside the compatibility `state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDashboardSnapshot {
    pub version: u32,
    pub host_id: String,
    pub host_kind: LocalRuntimeHostKind,
    pub pid: u32,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub resource_samples: Vec<LocalResourceSample>,
    pub components: Vec<LocalRuntimeComponentSnapshot>,
    pub log_location: String,
}
