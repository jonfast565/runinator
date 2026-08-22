use super::*;
use runinator_models::notifications::{NotificationDelivery, NotificationPolicy};
use uuid::Uuid;

pub async fn fetch_notification_policies<T: NotificationStore>(
    db: &T,
    workflow_id: Option<Uuid>,
) -> Result<Vec<NotificationPolicy>, SendableError> {
    db.fetch_notification_policies(workflow_id).await
}

pub async fn fetch_notification_policy<T: NotificationStore>(
    db: &T,
    policy_id: Uuid,
) -> Result<Option<NotificationPolicy>, SendableError> {
    db.fetch_notification_policy(policy_id).await
}

pub async fn create_notification_policy<T: NotificationStore>(
    db: &T,
    policy: &NewNotificationPolicy,
) -> Result<NotificationPolicy, SendableError> {
    validate_policy(policy)?;
    db.create_notification_policy(policy).await
}

pub async fn update_notification_policy<T: NotificationStore>(
    db: &T,
    policy_id: Uuid,
    policy: &NewNotificationPolicy,
) -> Result<Option<NotificationPolicy>, SendableError> {
    validate_policy(policy)?;
    db.update_notification_policy(policy_id, policy).await
}

pub async fn delete_notification_policy<T: NotificationStore>(
    db: &T,
    policy_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_notification_policy(policy_id).await
}

pub async fn fetch_notification_deliveries<T: NotificationStore>(
    db: &T,
    notification_id: Uuid,
) -> Result<Vec<NotificationDelivery>, SendableError> {
    db.fetch_notification_deliveries(notification_id).await
}

/// reject the policy shapes that would be stored happily but could never fire, so the failure is
/// visible at write time rather than as silence during an incident.
fn validate_policy(policy: &NewNotificationPolicy) -> Result<(), SendableError> {
    if policy.name.trim().is_empty() {
        return Err(crate::errors::NOTIFY_MISSING_TARGET.error("policy name is required"));
    }
    // an external channel with no target has nowhere to deliver.
    if policy.channel != NotificationChannel::InApp
        && policy
            .target
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err(crate::errors::NOTIFY_MISSING_TARGET.error(format!(
            "channel '{}' requires a target",
            policy.channel.as_str()
        )));
    }
    // a run-duration event is evaluated against a required threshold; secret expiry separately has
    // a default window when its scanner-driven policy omits one.
    if policy.event.is_duration_based() && policy.threshold_seconds.unwrap_or(0) <= 0 {
        return Err(crate::errors::NOTIFY_UNROUTABLE_CHANNEL.error(format!(
            "event '{}' requires a positive threshold_seconds",
            policy.event.as_str()
        )));
    }
    if policy.event == NotificationEvent::SecretExpiring {
        if policy.workflow_id.is_some() {
            return Err(crate::errors::NOTIFY_UNROUTABLE_CHANNEL
                .error("event 'secret_expiring' requires a global policy"));
        }
        if policy.threshold_seconds.is_some_and(|seconds| seconds <= 0) {
            return Err(crate::errors::NOTIFY_UNROUTABLE_CHANNEL.error(
                "event 'secret_expiring' threshold_seconds must be positive when provided",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "notification_policies_tests.rs"]
mod tests;
