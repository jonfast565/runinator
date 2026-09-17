#[allow(unused_imports)]
use super::*;

/// One immutable event in an admission's ordered timeline.  Result references are filled as the
/// event starts (or is promoted into) a generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressInboxEntry {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub sequence: i64,
    pub generation: i64,
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
    pub occurred_at: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub disposition: IngressEventDisposition,
    pub queue_state: IngressQueueState,
    pub queue_position: Option<i64>,
    pub promoted_generation: Option<i64>,
    pub workflow_run_id: Option<Uuid>,
    pub pipeline_run_id: Option<Uuid>,
}
