#[allow(unused_imports)]
use super::*;

/// a per-run, per-node gate: a workflow blocks on it until its status reaches `open`/`passed`.
/// distinct from an `ApprovalRequest` (a human decision) — a gate is an automated/policy check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub kind: GateKind,
    pub status: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub condition: Value,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
