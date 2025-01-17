use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: Option<i64>, 
    pub name: String, 
    pub cron_schedule: String, 
    pub action_name: String, 
    pub action_function: String,
    pub action_configuration: String, 
    pub timeout: i64, 
    pub next_execution: Option<DateTime<Utc>>,
    pub enabled: bool
}

#[derive(Debug, Serialize)]
pub struct TaskRun {
    pub id: i64,
    pub task_id: i64,
    pub start_time: i64,
    pub duration_ms: i64,
}
