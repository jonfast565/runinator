#[allow(unused_imports)]
use super::*;

/// Immutable inputs for one external notification delivery and its provider-effect outbox row.
#[derive(Debug, Clone)]
pub struct NewNotificationDelivery {
    pub id: Uuid,
    pub notification_id: Uuid,
    pub policy_id: Option<Uuid>,
    pub channel: NotificationChannel,
    pub provider: Option<String>,
    pub function: Option<String>,
    pub target: Option<String>,
    pub workflow_run_id: Option<Uuid>,
    pub command: EffectCommand,
}
