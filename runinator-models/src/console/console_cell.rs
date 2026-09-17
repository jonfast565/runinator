#[allow(unused_imports)]
use super::*;

/// one cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleCell {
    pub id: Uuid,
    pub session_id: Uuid,
    /// ordering within the session.
    pub position: i64,
    /// the name this cell's result binds to, if the author gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ConsoleCellKind>,
    pub status: ConsoleCellStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// set only for a cell that became a scratch workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
