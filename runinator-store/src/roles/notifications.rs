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
        NewNotification, NewNotificationPolicy, Notification, NotificationChannel,
        NotificationDelivery, NotificationDeliveryStatus, NotificationEvent, NotificationPolicy,
    },
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::runtime_store::RuntimeStore;

/// Core persistence operations for Runinator.
/// Notifications, the policies that raise them, and per-channel delivery attempts.
pub trait NotificationStore: Send + Sync + 'static {
    /// Operational snapshot of notification deliveries awaiting settlement.
    fn notification_delivery_queue_snapshot(
        &self,
    ) -> impl Future<Output = Result<QueueSnapshot, SendableError>> + Send;

    /// Persist a notification record.
    fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> impl Future<Output = Result<Notification, SendableError>> + Send;

    /// Fetch notifications, optionally only unread, most-recent first.
    fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Notification>, SendableError>> + Send;

    /// Mark a notification as read; returns the updated row.
    fn mark_notification_read(
        &self,
        notification_id: Uuid,
    ) -> impl Future<Output = Result<Option<Notification>, SendableError>> + Send;

    /// Mark all unread notifications as read; returns the number updated.
    fn mark_all_notifications_read(
        &self,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Delete a notification; returns true when a row was removed.
    fn delete_notification(
        &self,
        notification_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Persist a notification only if its dedupe key is unclaimed; `None` means one already exists.
    fn create_notification_if_absent(
        &self,
        notification: &NewNotification,
    ) -> impl Future<Output = Result<Option<Notification>, SendableError>> + Send;

    /// List notification policies, optionally narrowed to one workflow's own policies.
    fn fetch_notification_policies(
        &self,
        workflow_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<NotificationPolicy>, SendableError>> + Send;

    fn fetch_notification_policy(
        &self,
        policy_id: Uuid,
    ) -> impl Future<Output = Result<Option<NotificationPolicy>, SendableError>> + Send;

    /// Fetch the enabled policies that apply to a workflow for one event: the workflow's own plus
    /// the global (`workflow_id IS NULL`) ones.
    fn fetch_matching_notification_policies(
        &self,
        event: NotificationEvent,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<NotificationPolicy>, SendableError>> + Send;

    /// Fetch every enabled policy for a scanner-driven event, across all workflows.
    fn fetch_notification_policies_by_event(
        &self,
        event: NotificationEvent,
    ) -> impl Future<Output = Result<Vec<NotificationPolicy>, SendableError>> + Send;

    fn create_notification_policy(
        &self,
        policy: &NewNotificationPolicy,
    ) -> impl Future<Output = Result<NotificationPolicy, SendableError>> + Send;

    fn update_notification_policy(
        &self,
        policy_id: Uuid,
        policy: &NewNotificationPolicy,
    ) -> impl Future<Output = Result<Option<NotificationPolicy>, SendableError>> + Send;

    fn delete_notification_policy(
        &self,
        policy_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Replace a workflow's pack-managed policies wholesale, the way managed triggers reconcile:
    /// policies carrying `managed_by` are deleted and re-created, hand-authored ones are untouched.
    fn replace_managed_notification_policies(
        &self,
        workflow_id: Uuid,
        managed_by: String,
        policies: Vec<NewNotificationPolicy>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Record an external-channel delivery attributed to a notification.
    fn create_notification_delivery(
        &self,
        delivery_id: Uuid,
        notification_id: Uuid,
        policy_id: Option<Uuid>,
        channel: NotificationChannel,
        target: Option<String>,
        command: EffectCommand,
    ) -> impl Future<Output = Result<NotificationDelivery, SendableError>> + Send;

    /// Lease frozen notification provider effects for publication. Notification delivery has an
    /// independent durable outbox and never reuses the removed workflow action-dispatch table.
    fn claim_pending_notification_effect_dispatches(
        &self,
        publisher_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<NotificationEffectDispatchRecord>, SendableError>> + Send;

    fn mark_notification_effect_dispatch_published(
        &self,
        delivery_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn mark_notification_effect_dispatch_failed(
        &self,
        delivery_id: Uuid,
        error: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Settle a delivery after the worker reported back.
    fn mark_notification_delivery(
        &self,
        delivery_id: Uuid,
        status: NotificationDeliveryStatus,
        error: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// List deliveries for a notification, newest first.
    fn fetch_notification_deliveries(
        &self,
        notification_id: Uuid,
    ) -> impl Future<Output = Result<Vec<NotificationDelivery>, SendableError>> + Send;
}
