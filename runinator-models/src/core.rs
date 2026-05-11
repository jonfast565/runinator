use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_json_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: Option<i64>,
    pub name: String,
    pub cron_schedule: String,
    pub action_name: String,
    pub action_function: String,
    pub timeout: i64,
    pub next_execution: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub immediate: bool,
    pub blackout_start: Option<DateTime<Utc>>,
    pub blackout_end: Option<DateTime<Utc>>,
    #[serde(default = "default_json_object")]
    pub default_parameters: Value,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default = "default_json_object")]
    pub metadata: Value,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskRun {
    pub id: i64,
    pub task_id: i64,
    pub start_time: i64,
    pub duration_ms: i64,
}
