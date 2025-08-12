use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    pub id: Option<i64>,
    pub name: String,
    pub cron_schedule: String,
    pub action_name: String,
    pub action_configuration: Vec<u8>,
    pub timeout: i64,
    pub next_execution: Option<DateTime<Utc>>,
}
