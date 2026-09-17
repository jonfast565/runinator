#[allow(unused_imports)]
use super::*;

/// a declarative rule mapping a runtime failure condition to a severity and a delivery channel.
/// `workflow_id = None` makes the policy global (every workflow); pack-managed policies carry
/// `managed_by = "rexrap"` and are reconciled wholesale on import, the same way triggers are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPolicy {
    pub id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    pub event: NotificationEvent,
    #[serde(default)]
    pub severity: NotificationSeverity,
    #[serde(default)]
    pub channel: NotificationChannel,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub target: Option<String>,
    /// threshold for duration events, or the warning window for `secret_expiring`.
    /// `secret_expiring` defaults to the engine's 30-day window when omitted.
    #[serde(default)]
    pub threshold_seconds: Option<i64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub managed_by: Option<String>,
    #[serde(default)]
    pub configuration: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
