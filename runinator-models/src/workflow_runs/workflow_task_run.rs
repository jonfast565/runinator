#[allow(unused_imports)]
use super::*;

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
