use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::replicas::{TriggerActorType, TriggerSourceKind};
use crate::value::Value;
use crate::workflow_state::WorkflowExecutionState;
use crate::workflows::{WorkflowAction, WorkflowDefinition, WorkflowStatus};

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
    /// legacy migration carrier. new writes clear this column and never treat it as authoritative.
    #[serde(skip_serializing, default)]
    pub state: Value,
    /// optimistic-concurrency guard for `state`. bumped by every write that touches the blob;
    /// a compare-and-swap writer passes the value it read and retries when the row has moved on.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRun {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub status: WorkflowStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub state: Value,
    pub transition_reason: Option<String>,
    /// the node run created immediately before this one in the same workflow run, forming a flat,
    /// guid-linked execution chain that is easier to debug than the nested `steps` output tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_node_run_id: Option<Uuid>,
    /// the thread of control that produced this node run, so a run with fan-out can attribute each
    /// step to a branch instead of inferring it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
    /// true when a debugger "what if" cursor produced this. persisted independently of the cursor
    /// because a retired speculative cursor is gone from run state and this answer must outlive it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub speculative: bool,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_released_at: Option<DateTime<Utc>>,
}

/// A provider action launched by a workflow without holding that workflow's cursor.
///
/// A task run owns the worker lease and result independently of its launching node. The launcher
/// records this id in its output as the durable RexRap `task[T]` handle; an `await` node then joins
/// this record rather than re-driving the launch node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskRun {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub launch_node_run_id: Uuid,
    pub node_id: String,
    pub action: WorkflowAction,
    pub status: WorkflowStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunChunk {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub sequence: i64,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunArtifact {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

/// Input for promoting a node artifact to a run-level artifact via an output node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWorkflowRunArtifact {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
}

/// A run-level artifact declared by an output node, making it visible at workflow-run scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunArtifact {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
