use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Option<i64>,
    pub name: String,
    pub version: i64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub definition: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Queued,
    Running,
    Waiting,
    ApprovalRequired,
    Blocked,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl WorkflowStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowStatus::Queued => "queued",
            WorkflowStatus::Running => "running",
            WorkflowStatus::Waiting => "waiting",
            WorkflowStatus::ApprovalRequired => "approval_required",
            WorkflowStatus::Blocked => "blocked",
            WorkflowStatus::Succeeded => "succeeded",
            WorkflowStatus::Failed => "failed",
            WorkflowStatus::TimedOut => "timed_out",
            WorkflowStatus::Canceled => "canceled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            WorkflowStatus::Succeeded
                | WorkflowStatus::Failed
                | WorkflowStatus::TimedOut
                | WorkflowStatus::Canceled
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            WorkflowStatus::Queued
                | WorkflowStatus::Running
                | WorkflowStatus::Waiting
                | WorkflowStatus::ApprovalRequired
        )
    }
}

impl TryFrom<&str> for WorkflowStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(WorkflowStatus::Queued),
            "running" => Ok(WorkflowStatus::Running),
            "waiting" => Ok(WorkflowStatus::Waiting),
            "approval_required" => Ok(WorkflowStatus::ApprovalRequired),
            "blocked" => Ok(WorkflowStatus::Blocked),
            "succeeded" => Ok(WorkflowStatus::Succeeded),
            "failed" => Ok(WorkflowStatus::Failed),
            "timed_out" => Ok(WorkflowStatus::TimedOut),
            "canceled" => Ok(WorkflowStatus::Canceled),
            other => Err(format!("Unknown workflow status '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeKind {
    Start,
    Task,
    Wait,
    Condition,
    Approval,
    Loop,
    Subflow,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRetry {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i64,
}

impl Default for WorkflowRetry {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
        }
    }
}

fn default_max_attempts() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowTransitions {
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub on_success: Option<String>,
    #[serde(default)]
    pub on_failure: Option<String>,
    #[serde(default)]
    pub on_timeout: Option<String>,
    #[serde(default)]
    pub on_reject: Option<String>,
    #[serde(default)]
    pub branches: Vec<WorkflowBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBranch {
    pub when: Value,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,
    #[serde(default)]
    pub task_id: Option<i64>,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub wait: Value,
    #[serde(default)]
    pub condition: Value,
    #[serde(default)]
    pub transitions: WorkflowTransitions,
    #[serde(default)]
    pub retry: WorkflowRetry,
    #[serde(default)]
    pub timeout_seconds: Option<i64>,
    #[serde(default)]
    pub max_iterations: Option<i64>,
    #[serde(default)]
    pub subflow_id: Option<i64>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    pub workflow_id: i64,
    pub status: WorkflowStatus,
    pub active_node_id: Option<String>,
    pub parameters: Value,
    pub state: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRun {
    pub id: i64,
    pub workflow_run_id: i64,
    pub node_id: String,
    pub task_run_id: Option<i64>,
    pub status: WorkflowStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub state: Value,
    pub transition_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}
