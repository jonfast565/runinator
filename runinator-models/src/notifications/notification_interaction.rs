#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationInteraction {
    pub id: Uuid,
    pub notification_id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub target: NotificationInteractionTarget,
    pub actions: Vec<NotificationInteractionAction>,
    pub state: NotificationInteractionState,
    #[serde(default)]
    pub resolved_action: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<Uuid>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
