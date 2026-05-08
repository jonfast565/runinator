use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runs::RunStatus;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub task_id: i64,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub mappings: Vec<WorkflowMapping>,
    #[serde(default)]
    pub retry: WorkflowRetry,
    #[serde(default)]
    pub retry_count: i64,
    #[serde(default)]
    pub timeout: Option<i64>,
    #[serde(default)]
    pub timeout_seconds: Option<i64>,
    #[serde(default)]
    pub condition: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMapping {
    pub from_step: String,
    pub from_pointer: String,
    pub to_pointer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    pub workflow_id: i64,
    pub status: RunStatus,
    pub parameters: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepRun {
    pub id: i64,
    pub workflow_run_id: i64,
    pub step_id: String,
    pub task_run_id: Option<i64>,
    pub status: RunStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}
