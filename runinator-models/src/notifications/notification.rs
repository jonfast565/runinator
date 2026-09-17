#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub source_resource_type: Option<ResourceType>,
    #[serde(default)]
    pub source_resource_id: Option<Uuid>,
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
    pub interaction: Option<NotificationInteraction>,
    #[serde(default)]
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
