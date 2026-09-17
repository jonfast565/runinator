#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRuntimeSnapshot {
    pub host_kind: LocalRuntimeHostKind,
    pub pid: u32,
    pub started_at: String,
    pub updated_at: String,
    pub components: Vec<LocalRuntimeComponentSnapshot>,
}
