use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_node_id: Option<String>,
    pub channel: String,
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotification {
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_node_id: Option<String>,
    pub channel: String,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

fn default_severity() -> String {
    "info".to_string()
}
