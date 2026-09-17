#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ReplicaQuery {
    pub replica_type: Option<runinator_models::replicas::ReplicaKind>,
    pub status: Option<ReplicaStatus>,
}
