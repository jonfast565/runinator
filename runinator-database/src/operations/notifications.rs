//! notifications, policies, and deliveries.
//!
//! the `NotificationStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> NotificationStore for SqlStore<B>
where
    B: SqlBackend,
    // encode bounds for every bound value type.
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Vec<u8>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    // decode bounds (operations read a couple of columns directly; mappers read the rest).
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    // row indexing + executor plumbing.
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let columns = "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO notifications (id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(notification.workflow_run_id)
            .bind(notification.workflow_node_id.clone())
            .bind(notification.channel.as_str())
            .bind(notification.severity.as_str())
            .bind(notification.title.as_str())
            .bind(notification.body.clone())
            .bind(notification.target.clone())
            .bind(notification.metadata.to_string())
            .bind(notification.dedupe_key.clone())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM notifications WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_notification(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO notifications (id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(notification.workflow_run_id)
        .bind(notification.workflow_node_id.clone())
        .bind(notification.channel.as_str())
        .bind(notification.severity.as_str())
        .bind(notification.title.as_str())
        .bind(notification.body.clone())
        .bind(notification.target.clone())
        .bind(notification.metadata.to_string())
        .bind(notification.dedupe_key.clone())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_notification(&row))
    }

    async fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        let bounded_limit = limit.clamp(1, 1000);
        let rows = if unread_only {
            sqlx::query(&self.render(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE read_at IS NULL ORDER BY created_at DESC LIMIT ?",
            ))
            .bind(bounded_limit)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(&self.render(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications ORDER BY created_at DESC LIMIT ?",
            ))
            .bind(bounded_limit)
            .fetch_all(self.pool())
            .await?
        };
        Ok(rows.iter().map(mappers::row_to_notification).collect())
    }

    async fn mark_notification_read(
        &self,
        notification_id: Uuid,
    ) -> Result<Option<Notification>, SendableError> {
        let columns = "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";

        // mysql has no UPDATE ... RETURNING, so update then read the row back by id.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(
                &self
                    .render("UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ?"),
            )
            .bind(Utc::now().timestamp())
            .bind(notification_id)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM notifications WHERE id = ?",)),
            )
            .bind(notification_id)
            .fetch_optional(self.pool())
            .await?;
            return Ok(row.map(|row| mappers::row_to_notification(&row)));
        }

        let row = sqlx::query(&self.render(&format!(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ? RETURNING {columns}",
        )))
        .bind(Utc::now().timestamp())
        .bind(notification_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification(&row)))
    }

    async fn mark_all_notifications_read(&self) -> Result<u64, SendableError> {
        let result =
            sqlx::query(&self.render("UPDATE notifications SET read_at = ? WHERE read_at IS NULL"))
                .bind(Utc::now().timestamp())
                .execute(self.pool())
                .await?;
        Ok(result.affected())
    }

    async fn delete_notification(&self, notification_id: Uuid) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("DELETE FROM notifications WHERE id = ?"))
            .bind(notification_id)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }

    async fn create_notification_if_absent(
        &self,
        notification: &NewNotification,
    ) -> Result<Option<Notification>, SendableError> {
        let Some(dedupe_key) = notification.dedupe_key.clone() else {
            return self.create_notification(notification).await.map(Some);
        };
        let columns = "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        let sql = queries::insert_ignore(
            self.dialect(),
            "notifications",
            "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at",
            "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?",
            "dedupe_key",
            None,
        );
        let result = sqlx::query(&self.render(&sql))
            .bind(id)
            .bind(notification.workflow_run_id)
            .bind(notification.workflow_node_id.clone())
            .bind(notification.channel.as_str())
            .bind(notification.severity.as_str())
            .bind(notification.title.as_str())
            .bind(notification.body.clone())
            .bind(notification.target.clone())
            .bind(notification.metadata.to_string())
            .bind(dedupe_key.clone())
            .bind(created_at)
            .execute(self.pool())
            .await?;
        // no row inserted means another replica already emitted for this key; the caller skips.
        if result.affected() == 0 {
            return Ok(None);
        }
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notifications WHERE dedupe_key = ?"
        )))
        .bind(dedupe_key)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification(&row)))
    }

    async fn fetch_notification_policies(
        &self,
        workflow_id: Option<Uuid>,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let rows = match workflow_id {
            Some(workflow_id) => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM notification_policies WHERE workflow_id = ? ORDER BY created_at DESC"
                )))
                .bind(workflow_id)
                .fetch_all(self.pool())
                .await?
            }
            None => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM notification_policies ORDER BY created_at DESC"
                )))
                .fetch_all(self.pool())
                .await?
            }
        };
        Ok(rows
            .iter()
            .map(mappers::row_to_notification_policy)
            .collect())
    }

    async fn fetch_matching_notification_policies(
        &self,
        event: NotificationEvent,
        workflow_id: Uuid,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let enabled = queries::bool_true(self.dialect());
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies
             WHERE enabled = {enabled} AND event = ? AND (workflow_id = ? OR workflow_id IS NULL)
             ORDER BY created_at",
        )))
        .bind(event.as_str())
        .bind(workflow_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_notification_policy)
            .collect())
    }

    async fn fetch_notification_policies_by_event(
        &self,
        event: NotificationEvent,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let enabled = queries::bool_true(self.dialect());
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies
             WHERE enabled = {enabled} AND event = ? ORDER BY created_at",
        )))
        .bind(event.as_str())
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_notification_policy)
            .collect())
    }

    async fn create_notification_policy(
        &self,
        policy: &NewNotificationPolicy,
    ) -> Result<NotificationPolicy, SendableError> {
        let id = Uuid::now_v7();
        self.insert_notification_policy(id, policy).await?;
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_notification_policy(&row))
    }

    async fn update_notification_policy(
        &self,
        policy_id: Uuid,
        policy: &NewNotificationPolicy,
    ) -> Result<Option<NotificationPolicy>, SendableError> {
        let updated_at = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE notification_policies
             SET workflow_id = ?, name = ?, event = ?, severity = ?, channel = ?, target = ?,
                 threshold_seconds = ?, enabled = ?, managed_by = ?, configuration = ?, updated_at = ?
             WHERE id = ?",
        ))
        .bind(policy.workflow_id)
        .bind(policy.name.as_str())
        .bind(policy.event.as_str())
        .bind(policy.severity.as_str())
        .bind(policy.channel.as_str())
        .bind(policy.target.clone())
        .bind(policy.threshold_seconds)
        .bind(policy.enabled)
        .bind(policy.managed_by.clone())
        .bind(policy.configuration.to_string())
        .bind(updated_at)
        .bind(policy_id)
        .execute(self.pool())
        .await?;
        if result.affected() == 0 {
            return Ok(None);
        }
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies WHERE id = ?"
        )))
        .bind(policy_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification_policy(&row)))
    }

    async fn delete_notification_policy(&self, policy_id: Uuid) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("DELETE FROM notification_policies WHERE id = ?"))
            .bind(policy_id)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }

    async fn replace_managed_notification_policies(
        &self,
        workflow_id: Uuid,
        managed_by: String,
        policies: Vec<NewNotificationPolicy>,
    ) -> Result<(), SendableError> {
        // drop only this manager's rows so hand-authored policies on the same workflow survive an
        // import, matching how managed triggers reconcile.
        sqlx::query(
            &self.render(
                "DELETE FROM notification_policies WHERE workflow_id = ? AND managed_by = ?",
            ),
        )
        .bind(workflow_id)
        .bind(managed_by.as_str())
        .execute(self.pool())
        .await?;
        for policy in policies {
            self.insert_notification_policy(Uuid::now_v7(), &policy)
                .await?;
        }
        Ok(())
    }

    async fn create_notification_delivery(
        &self,
        notification_id: Uuid,
        policy_id: Option<Uuid>,
        channel: NotificationChannel,
        target: Option<String>,
    ) -> Result<NotificationDelivery, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO notification_deliveries (id, notification_id, policy_id, channel, target, status, attempts, last_error, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 0, NULL, ?, ?)",
        ))
        .bind(id)
        .bind(notification_id)
        .bind(policy_id)
        .bind(channel.as_str())
        .bind(target)
        .bind(NotificationDeliveryStatus::Pending.as_str())
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        let columns = NOTIFICATION_DELIVERY_COLUMNS;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_deliveries WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_notification_delivery(&row))
    }

    async fn mark_notification_delivery(
        &self,
        delivery_id: Uuid,
        status: NotificationDeliveryStatus,
        error: Option<String>,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "UPDATE notification_deliveries
             SET status = ?, attempts = attempts + 1, last_error = ?, updated_at = ?
             WHERE id = ?",
        ))
        .bind(status.as_str())
        .bind(error)
        .bind(Utc::now().timestamp())
        .bind(delivery_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_notification_deliveries(
        &self,
        notification_id: Uuid,
    ) -> Result<Vec<NotificationDelivery>, SendableError> {
        let columns = NOTIFICATION_DELIVERY_COLUMNS;
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_deliveries WHERE notification_id = ? ORDER BY created_at DESC"
        )))
        .bind(notification_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_notification_delivery)
            .collect())
    }
}
