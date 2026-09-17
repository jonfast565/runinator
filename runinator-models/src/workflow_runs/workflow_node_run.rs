#[allow(unused_imports)]
use super::*;

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
