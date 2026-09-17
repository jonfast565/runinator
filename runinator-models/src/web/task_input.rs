#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub cron_schedule: String,
    pub action_name: String,
    pub timeout: i64,
    pub next_execution: Option<DateTime<Utc>>,
}
