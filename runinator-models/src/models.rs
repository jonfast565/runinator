use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: Option<i64>,            // Task ID for database
    pub name: String,               // Task name
    pub cron_schedule: String,      // Cron expression
    pub action_name: String,        // Name of the action in the DLL
    pub action_configuration: Vec<u8>, // Serialized binary data to be passed to the DLL
    pub timeout: i64,               // Timeout in minutes
    pub next_execution: Option<DateTime<Utc>>, // Next execution time
}

#[derive(Debug, Serialize)]
pub struct TaskRun {
    pub id: i64,
    pub task_name: String,
    pub start_time: i64,
    pub duration_ms: i64,
}