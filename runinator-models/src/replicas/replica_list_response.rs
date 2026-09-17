#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaListResponse {
    pub counts: ReplicaCounts,
    pub replicas: Vec<ReplicaRecord>,
    /// number of node runs currently executing on each replica, keyed by replica id. derived by the
    /// web service from live executor claims, so only replicas actually running tasks appear.
    #[serde(default)]
    pub running_tasks: std::collections::HashMap<Uuid, i64>,
}
