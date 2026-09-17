#[allow(unused_imports)]
use super::*;

/// a replica's recent telemetry samples, oldest first, for charting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaSampleSeries {
    pub replica_id: uuid::Uuid,
    pub samples: Vec<ReplicaSample>,
}
