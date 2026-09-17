#[allow(unused_imports)]
use super::*;

/// one authored node's program, frozen between the durable calls it makes.
///
/// the module is deliberately absent: it lives in the run's workflow snapshot, which already
/// insulates an in-flight run from a redefinition. only the version is stored, so a resume can
/// refuse a continuation the current module would misread rather than silently running the wrong
/// instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInvocation {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    /// the node run this invocation *is*. one node run spans every call, which is the whole point:
    /// retries, logs and artifacts stay attributed to the authored node.
    pub workflow_node_run_id: Uuid,
    /// the thread of control that owns it. a fan-out can put two invocations on one node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
    pub node_id: String,
    pub module_version: u32,
    pub continuation: InvocationContinuation,
    pub status: WorkflowStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}
