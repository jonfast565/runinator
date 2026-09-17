use crate::auth::ResourceType;
use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text,
};

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

mod notification;
pub use notification::Notification;

mod new_notification;
pub use new_notification::NewNotification;

mod notification_policy;
pub use notification_policy::NotificationPolicy;

mod new_notification_policy;
pub use new_notification_policy::NewNotificationPolicy;

mod notification_delivery;
pub use notification_delivery::NotificationDelivery;

mod notification_interaction_action;
pub use notification_interaction_action::NotificationInteractionAction;

mod notification_interaction;
pub use notification_interaction::NotificationInteraction;

mod conversation_receipt;
pub use conversation_receipt::ConversationReceipt;

mod external_interaction_response;
pub use external_interaction_response::ExternalInteractionResponse;

mod notification_interaction_action_request;
pub use notification_interaction_action_request::NotificationInteractionActionRequest;
