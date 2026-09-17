//! notifications, the policies that raise them, and per-channel delivery attempts.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use super::QueueSnapshot;
use chrono::{DateTime, Utc};
use runinator_comm::{EffectCommand, NotificationEffectDispatchRecord};
use uuid::Uuid;

use runinator_models::{
    errors::SendableError,
    notifications::{
        ConversationReceipt, NewNotification, NewNotificationPolicy, Notification,
        NotificationChannel, NotificationDelivery, NotificationDeliveryStatus, NotificationEvent,
        NotificationInteraction, NotificationInteractionAction, NotificationInteractionTarget,
        NotificationPolicy,
    },
};

/// Core persistence operations for Runinator.
/// Notifications, the policies that raise them, and per-channel delivery attempts.
mod new_notification_delivery;
pub use new_notification_delivery::NewNotificationDelivery;

mod new_notification_interaction;
pub use new_notification_interaction::NewNotificationInteraction;

mod notification_store;
pub use notification_store::NotificationStore;
