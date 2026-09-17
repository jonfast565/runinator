#[allow(unused_imports)]
use super::*;

/// a first-class pipeline execution. an orchestration envelope over the member workflow runs it
/// starts: each member run is stamped with this run's id, and the run settles when the reachable
/// member graph reaches terminal. status reuses [`WorkflowStatus`] (queued, running, parked,
/// sleeping, and the terminal states are meaningful for a pipeline run).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    #[serde(default)]
    pub pipeline_snapshot: Option<Pipeline>,
    pub status: WorkflowStatus,
    pub parameters: Value,
    pub state: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_display_name: Option<String>,
    #[serde(default)]
    pub trigger_metadata: Value,
    /// Present when this immutable run is one epoch of a correlated orchestration binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_binding_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_epoch: Option<i64>,
    /// Optional member chosen as the sole initial frontier for a resumed/superseding epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_member: Option<String>,
}
