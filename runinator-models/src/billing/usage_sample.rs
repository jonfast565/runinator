#[allow(unused_imports)]
use super::*;

/// a point-in-time sample of an org's running node count for a (backend, kind), used to accrue
/// node-hours between samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSample {
    pub org_id: Uuid,
    pub backend: ProvisionBackend,
    pub kind: ReplicaKind,
    pub node_count: u32,
    pub sampled_at: DateTime<Utc>,
}
