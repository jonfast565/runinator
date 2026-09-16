use crate::auth::ResourceType;
use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotification {
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
    /// a settings-store secret entered its configured ahead-of-expiry warning window.
    SecretExpiring,
}

impl NotificationEvent {
    pub const ALL: [NotificationEvent; 5] = [
        NotificationEvent::RunFailed,
        NotificationEvent::NodeRetryExhausted,
        NotificationEvent::RunSlaBreached,
        NotificationEvent::RunParked,
        NotificationEvent::SecretExpiring,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationEvent::RunFailed => "run_failed",
            NotificationEvent::NodeRetryExhausted => "node_retry_exhausted",
            NotificationEvent::RunSlaBreached => "run_sla_breached",
            NotificationEvent::RunParked => "run_parked",
            NotificationEvent::SecretExpiring => "secret_expiring",
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
            "secret_expiring" => Ok(NotificationEvent::SecretExpiring),
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
            "warning" => Ok(NotificationSeverity::Warning),
            "critical" => Ok(NotificationSeverity::Critical),
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotificationPolicy {
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
    #[serde(default)]
    pub threshold_seconds: Option<i64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub managed_by: Option<String>,
    #[serde(default)]
    pub configuration: Value,
}

impl Validate for NewNotification {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("channel", &self.channel, SHORT_TEXT_MAX)?;
        required_text("severity", &self.severity, SHORT_TEXT_MAX)?;
        required_text("title", &self.title, SHORT_TEXT_MAX)?;
        optional_text("body", self.body.as_deref(), LONG_TEXT_MAX)?;
        optional_text("target", self.target.as_deref(), 2 * 1024)?;
        optional_text("dedupe_key", self.dedupe_key.as_deref(), SHORT_TEXT_MAX)
    }
}

impl Validate for NewNotificationPolicy {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        optional_text("target", self.target.as_deref(), 2 * 1024)?;
        optional_text("managed_by", self.managed_by.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("provider", self.provider.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("function", self.function.as_deref(), SHORT_TEXT_MAX)?;
        if self.provider.is_some() != self.function.is_some() {
            return Err(ValidationError::new(
                "provider",
                "provider and function must be supplied together",
            ));
        }
        if let Some(provider) = &self.provider {
            required_text("provider", provider, SHORT_TEXT_MAX)?;
        }
        if let Some(function) = &self.function {
            required_text("function", function, SHORT_TEXT_MAX)?;
        }
        if let Some(seconds) = self.threshold_seconds
            && seconds <= 0
        {
            return Err(ValidationError::new(
                "threshold_seconds",
                "must be greater than zero",
            ));
        }
        if self.event.is_duration_based() && self.threshold_seconds.is_none() {
            return Err(ValidationError::new(
                "threshold_seconds",
                "is required for duration-based events",
            ));
        }
        Ok(())
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationInteractionState {
    Open,
    Resolved,
    Stale,
}

impl NotificationInteractionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Stale => "stale",
        }
    }
}

impl TryFrom<&str> for NotificationInteractionState {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "open" => Ok(Self::Open),
            "resolved" => Ok(Self::Resolved),
            "stale" => Ok(Self::Stale),
            other => Err(format!("unknown notification interaction state '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationInteractionInput {
    None,
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationInteractionAction {
    pub id: String,
    pub label: String,
    #[serde(default = "default_interaction_input")]
    pub input: NotificationInteractionInput,
}

fn default_interaction_input() -> NotificationInteractionInput {
    NotificationInteractionInput::None
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationInteractionTarget {
    Effect {
        workflow_run_id: Uuid,
        effect_id: Uuid,
        attempt: u32,
    },
    Signal {
        workflow_run_id: Uuid,
        effect_id: Uuid,
        attempt: u32,
        name: String,
    },
    Terminal {
        workflow_run_id: Uuid,
        effect_id: Uuid,
        attempt: u32,
    },
}

impl NotificationInteractionTarget {
    pub fn workflow_run_id(&self) -> Uuid {
        match self {
            Self::Effect {
                workflow_run_id, ..
            }
            | Self::Signal {
                workflow_run_id, ..
            }
            | Self::Terminal {
                workflow_run_id, ..
            } => *workflow_run_id,
        }
    }

    pub fn effect_id(&self) -> Uuid {
        match self {
            Self::Effect { effect_id, .. }
            | Self::Signal { effect_id, .. }
            | Self::Terminal { effect_id, .. } => *effect_id,
        }
    }

    pub fn attempt(&self) -> u32 {
        match self {
            Self::Effect { attempt, .. }
            | Self::Signal { attempt, .. }
            | Self::Terminal { attempt, .. } => *attempt,
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReceipt {
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalInteractionResponse {
    pub actor_subject: String,
    pub action_id: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationInteractionActionRequest {
    #[serde(default)]
    pub input: Value,
}

impl Validate for NotificationInteractionActionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
