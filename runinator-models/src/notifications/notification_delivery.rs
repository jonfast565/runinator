#[allow(unused_imports)]
use super::*;

/// one external-channel send attributed to a notification. tracked durably so a delivery that fails
/// in the worker is visible rather than lost, and so the result consumer has a row to settle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDelivery {
    pub id: Uuid,
    pub notification_id: Uuid,
    #[serde(default)]
    pub policy_id: Option<Uuid>,
    pub channel: NotificationChannel,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    pub status: NotificationDeliveryStatus,
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub response: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
