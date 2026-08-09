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
    /// stable key making engine-emitted notifications idempotent: a policy that keeps matching on
    /// every scan tick collapses onto one row instead of one per tick. `None` for manual posts.
    #[serde(default)]
    pub dedupe_key: Option<String>,
}

fn default_severity() -> String {
    "info".to_string()
}

/// the runtime condition a [`NotificationPolicy`] fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NotificationEvent {
    /// a workflow run reached a terminal failed/timed-out state.
    #[default]
    RunFailed,
    /// a node exhausted its retry policy without succeeding.
    NodeRetryExhausted,
    /// a run stayed open past its declared sla.
    RunSlaBreached,
    /// a run sat parked (waiting) past a threshold without progressing.
    RunParked,
}

impl NotificationEvent {
    pub const ALL: [NotificationEvent; 4] = [
        NotificationEvent::RunFailed,
        NotificationEvent::NodeRetryExhausted,
        NotificationEvent::RunSlaBreached,
        NotificationEvent::RunParked,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationEvent::RunFailed => "run_failed",
            NotificationEvent::NodeRetryExhausted => "node_retry_exhausted",
            NotificationEvent::RunSlaBreached => "run_sla_breached",
            NotificationEvent::RunParked => "run_parked",
        }
    }

    /// duration-based events are evaluated by the periodic scanner rather than at a transition, and
    /// require a threshold to be meaningful.
    pub fn is_duration_based(&self) -> bool {
        matches!(
            self,
            NotificationEvent::RunSlaBreached | NotificationEvent::RunParked
        )
    }
}

impl TryFrom<&str> for NotificationEvent {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "run_failed" => Ok(NotificationEvent::RunFailed),
            "node_retry_exhausted" => Ok(NotificationEvent::NodeRetryExhausted),
            "run_sla_breached" => Ok(NotificationEvent::RunSlaBreached),
            "run_parked" => Ok(NotificationEvent::RunParked),
            other => Err(format!("unknown notification event '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    #[default]
    Info,
    Warning,
    Critical,
}

impl NotificationSeverity {
    pub const ALL: [NotificationSeverity; 3] = [
        NotificationSeverity::Info,
        NotificationSeverity::Warning,
        NotificationSeverity::Critical,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationSeverity::Info => "info",
            NotificationSeverity::Warning => "warning",
            NotificationSeverity::Critical => "critical",
        }
    }
}

impl TryFrom<&str> for NotificationSeverity {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "info" => Ok(NotificationSeverity::Info),
            "warning" | "warn" => Ok(NotificationSeverity::Warning),
            "critical" | "error" => Ok(NotificationSeverity::Critical),
            other => Err(format!("unknown notification severity '{other}'")),
        }
    }
}

/// where a fired policy delivers. `InApp` is written straight to the notifications table; the rest
/// are handed to the normal provider execution path so the engine never speaks a vendor protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    #[default]
    InApp,
    Slack,
    Email,
}

impl NotificationChannel {
    pub const ALL: [NotificationChannel; 3] = [
        NotificationChannel::InApp,
        NotificationChannel::Slack,
        NotificationChannel::Email,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationChannel::InApp => "in_app",
            NotificationChannel::Slack => "slack",
            NotificationChannel::Email => "email",
        }
    }

    /// the provider crate that delivers this channel, or `None` when the engine persists it itself.
    pub fn provider(&self) -> Option<(&'static str, &'static str)> {
        match self {
            NotificationChannel::InApp => None,
            NotificationChannel::Slack => Some(("slack", "send_message")),
            NotificationChannel::Email => Some(("email", "send")),
        }
    }
}

impl TryFrom<&str> for NotificationChannel {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "in_app" | "inapp" | "ui" => Ok(NotificationChannel::InApp),
            "slack" => Ok(NotificationChannel::Slack),
            "email" | "mail" => Ok(NotificationChannel::Email),
            other => Err(format!("unknown notification channel '{other}'")),
        }
    }
}

/// a declarative rule mapping a runtime failure condition to a severity and a delivery channel.
/// `workflow_id = None` makes the policy global (every workflow); pack-managed policies carry
/// `managed_by = "wdl"` and are reconciled wholesale on import, the same way triggers are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPolicy {
    pub id: Uuid,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    pub event: NotificationEvent,
    #[serde(default)]
    pub severity: NotificationSeverity,
    #[serde(default)]
    pub channel: NotificationChannel,
    #[serde(default)]
    pub target: Option<String>,
    /// threshold for the duration-based events; ignored by transition-based ones.
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotificationPolicy {
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    pub event: NotificationEvent,
    #[serde(default)]
    pub severity: NotificationSeverity,
    #[serde(default)]
    pub channel: NotificationChannel,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub threshold_seconds: Option<i64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub managed_by: Option<String>,
    #[serde(default)]
    pub configuration: Value,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryStatus {
    /// persisted, not yet handed to the action outbox.
    #[default]
    Pending,
    /// dispatched to a worker through the action channel; awaiting its result.
    Dispatched,
    Delivered,
    Failed,
}

impl NotificationDeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationDeliveryStatus::Pending => "pending",
            NotificationDeliveryStatus::Dispatched => "dispatched",
            NotificationDeliveryStatus::Delivered => "delivered",
            NotificationDeliveryStatus::Failed => "failed",
        }
    }
}

impl TryFrom<&str> for NotificationDeliveryStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(NotificationDeliveryStatus::Pending),
            "dispatched" => Ok(NotificationDeliveryStatus::Dispatched),
            "delivered" => Ok(NotificationDeliveryStatus::Delivered),
            "failed" => Ok(NotificationDeliveryStatus::Failed),
            other => Err(format!("unknown notification delivery status '{other}'")),
        }
    }
}

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
    pub target: Option<String>,
    pub status: NotificationDeliveryStatus,
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
