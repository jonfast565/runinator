#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct NewNotificationInteraction {
    pub id: Uuid,
    pub notification_id: Uuid,
    pub org_id: Option<Uuid>,
    pub target: NotificationInteractionTarget,
    pub actions: Vec<NotificationInteractionAction>,
}
