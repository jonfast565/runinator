//! application service for notifications and notification policies.

use std::sync::Arc;

use runinator_broker_core::{AppEvent, AppEventKind, UiEventPublisher, emit};
use runinator_models::{
    errors::SendableError,
    notifications::{
        NewNotification, NewNotificationPolicy, Notification, NotificationDelivery,
        NotificationPolicy,
    },
};
use runinator_store::{RuntimeStore, roles::NotificationStore};
use uuid::Uuid;

use crate::repository;

/// Coordinates notification writes with their UI invalidations.
#[derive(Clone)]
pub struct NotificationOperations<T> {
    store: Arc<T>,
    events: UiEventPublisher,
}

impl<T> NotificationOperations<T> {
    pub fn new(store: Arc<T>, events: UiEventPublisher) -> Self {
        Self { store, events }
    }

    fn changed(&self) {
        emit(
            &self.events,
            AppEvent::global(AppEventKind::NotificationsChanged),
        );
    }
}

impl<T: RuntimeStore + NotificationStore> NotificationOperations<T> {
    pub async fn list(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        repository::fetch_notifications(self.store.as_ref(), unread_only, limit).await
    }

    pub async fn create(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let created = repository::create_notification(self.store.as_ref(), notification).await?;
        emit(
            &self.events,
            AppEvent::new(
                created.org_id,
                AppEventKind::NotificationCreated {
                    notification_id: created.notification.id,
                },
            ),
        );
        Ok(created.notification)
    }

    pub async fn mark_read(
        &self,
        notification_id: Uuid,
    ) -> Result<Option<Notification>, SendableError> {
        let notification =
            repository::mark_notification_read(self.store.as_ref(), notification_id).await?;
        if notification.is_some() {
            self.changed();
        }
        Ok(notification)
    }

    pub async fn delete(&self, notification_id: Uuid) -> Result<bool, SendableError> {
        let deleted = repository::delete_notification(self.store.as_ref(), notification_id).await?;
        if deleted {
            self.changed();
        }
        Ok(deleted)
    }

    pub async fn mark_all_read(&self) -> Result<u64, SendableError> {
        let count = repository::mark_all_notifications_read(self.store.as_ref()).await?;
        self.changed();
        Ok(count)
    }

    pub async fn list_policies(
        &self,
        workflow_id: Option<Uuid>,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        repository::fetch_notification_policies(self.store.as_ref(), workflow_id).await
    }

    pub async fn fetch_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<NotificationPolicy>, SendableError> {
        repository::fetch_notification_policy(self.store.as_ref(), policy_id).await
    }

    pub async fn create_policy(
        &self,
        policy: &NewNotificationPolicy,
    ) -> Result<NotificationPolicy, SendableError> {
        repository::create_notification_policy(self.store.as_ref(), policy).await
    }

    pub async fn update_policy(
        &self,
        policy_id: Uuid,
        policy: &NewNotificationPolicy,
    ) -> Result<Option<NotificationPolicy>, SendableError> {
        repository::update_notification_policy(self.store.as_ref(), policy_id, policy).await
    }

    pub async fn delete_policy(&self, policy_id: Uuid) -> Result<bool, SendableError> {
        repository::delete_notification_policy(self.store.as_ref(), policy_id).await
    }

    pub async fn deliveries(
        &self,
        notification_id: Uuid,
    ) -> Result<Vec<NotificationDelivery>, SendableError> {
        repository::fetch_notification_deliveries(self.store.as_ref(), notification_id).await
    }
}
