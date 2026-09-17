#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: Uuid,
    pub workflow_id: Uuid,
    #[serde(default)]
    pub workflow_snapshot: Option<WorkflowDefinition>,
    pub status: WorkflowStatus,
    pub active_node_id: Option<String>,
    pub parameters: Value,
    /// normalized execution state assembled from the workflow state tables.
    #[serde(skip)]
    pub execution_state: WorkflowExecutionState,
    /// Optimistic-concurrency guard for normalized execution state. A compare-and-swap writer
    /// passes the value it read and retries when another writer moved the run first.
    #[serde(default)]
    pub state_version: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// optional stable identity for this run used by `await workflow ... key` joins. set at start
    /// (trigger/api/subflow) or stamped by the engine from the workflow's `metadata.correlation`
    /// expression as the run progresses; write-once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_key: Option<String>,
    /// set when this run is a member of a pipeline run; the pipeline-run orchestrator uses it to
    /// aggregate member terminals and propagates it along in-pipeline chained links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_request_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_request_ip: Option<String>,
    #[serde(default)]
    pub trigger_metadata: Value,
}
