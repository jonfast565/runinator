use super::*;
use runinator_models::notifications::{NewNotification, Notification};
use uuid::Uuid;

/// a created notification together with the org that should see the UI event for it.
pub struct CreatedNotification {
    pub notification: Notification,
    pub org_id: Option<Uuid>,
}

pub async fn fetch_notifications<T: NotificationStore>(
    db: &T,
    unread_only: bool,
    limit: i64,
) -> Result<Vec<Notification>, SendableError> {
    db.fetch_notifications(unread_only, limit).await
}

/// persist a notification and resolve the org that owns it, so the caller emits the UI event to the
/// right audience rather than globally.
pub async fn create_notification<T: NotificationStore + RuntimeStore>(
    db: &T,
    notification: &NewNotification,
) -> Result<CreatedNotification, SendableError> {
    let notification = db.create_notification(notification).await?;
    let org_id = match notification.workflow_run_id {
        Some(workflow_run_id) => super::org_id_for_workflow_run(db, workflow_run_id).await,
        None => None,
    };
    Ok(CreatedNotification {
        notification,
        org_id,
    })
}

pub async fn mark_notification_read<T: NotificationStore>(
    db: &T,
    notification_id: Uuid,
) -> Result<Option<Notification>, SendableError> {
    db.mark_notification_read(notification_id).await
}

pub async fn mark_all_notifications_read<T: NotificationStore>(
    db: &T,
) -> Result<u64, SendableError> {
    db.mark_all_notifications_read().await
}

pub async fn delete_notification<T: NotificationStore>(
    db: &T,
    notification_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_notification(notification_id).await
}
