use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTriggerKind {
    Cron,
    Manual,
    /// fire when a source workflow run reaches a terminal state (workflow-to-workflow chaining).
    /// the trigger belongs to the source workflow; the target lives in `configuration`.
    Chained,
}

impl WorkflowTriggerKind {
    /// every trigger kind in a stable, UI-facing order.
    pub const ALL: [WorkflowTriggerKind; 3] = [
        WorkflowTriggerKind::Cron,
        WorkflowTriggerKind::Manual,
        WorkflowTriggerKind::Chained,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowTriggerKind::Cron => "cron",
            WorkflowTriggerKind::Manual => "manual",
            WorkflowTriggerKind::Chained => "chained",
        }
    }
}

impl TryFrom<&str> for WorkflowTriggerKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "cron" => Ok(WorkflowTriggerKind::Cron),
            "manual" => Ok(WorkflowTriggerKind::Manual),
            "chained" => Ok(WorkflowTriggerKind::Chained),
            other => Err(format!("Unknown workflow trigger kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    pub id: Option<Uuid>,
    pub workflow_id: Uuid,
    pub kind: WorkflowTriggerKind,
    pub enabled: bool,
    #[serde(default)]
    pub configuration: Value,
    pub next_execution: Option<DateTime<Utc>>,
    pub blackout_start: Option<DateTime<Utc>>,
    pub blackout_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}
