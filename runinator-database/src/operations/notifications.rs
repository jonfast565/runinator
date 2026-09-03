//! notifications, policies, and deliveries.
//!
//! the `NotificationStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

fn scoped_dedupe_key(org_id: Option<Uuid>, key: Option<&str>) -> Option<String> {
    key.map(|key| match org_id {
        Some(org_id) => format!("organization:{org_id}:{key}"),
        None => format!("platform:{key}"),
    })
}

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
    async fn notification_delivery_queue_snapshot(&self) -> Result<QueueSnapshot, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT COUNT(*) AS depth, MIN(created_at) AS oldest
             FROM notification_deliveries WHERE published_at IS NULL AND command_json IS NOT NULL",
        ))
        .fetch_one(self.pool())
        .await?;
        let oldest: Option<i64> = row.try_get("oldest")?;
        Ok(QueueSnapshot {
            depth: row.try_get::<i64, _>("depth")?.max(0) as u64,
            claimed: 0,
            oldest_enqueued_at: oldest.and_then(|value| DateTime::from_timestamp(value, 0)),
        })
    }

    async fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let columns = NOTIFICATION_COLUMNS;
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        let dedupe_key = scoped_dedupe_key(notification.org_id, notification.dedupe_key.as_deref());
        if self.dialect() == SqlDialect::MariaDb {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO notifications (id, org_id, source_resource_type, source_resource_id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(notification.org_id)
            .bind(notification.source_resource_type.map(|kind| kind.as_str().to_string()))
            .bind(notification.source_resource_id)
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
            "INSERT INTO notifications (id, org_id, source_resource_type, source_resource_id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(notification.org_id)
        .bind(notification.source_resource_type.map(|kind| kind.as_str().to_string()))
        .bind(notification.source_resource_id)
        .bind(notification.workflow_run_id)
        .bind(notification.workflow_node_id.clone())
        .bind(notification.channel.as_str())
        .bind(notification.severity.as_str())
        .bind(notification.title.as_str())
        .bind(notification.body.clone())
        .bind(notification.target.clone())
        .bind(notification.metadata.to_string())
        .bind(dedupe_key)
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_notification(&row))
    }

    async fn fetch_notifications(
        &self,
        org_id: Option<Uuid>,
        user_id: Uuid,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        let bounded_limit = limit.clamp(1, 1000);
        let unread = if unread_only {
            "AND COALESCE(r.read_at, n.read_at) IS NULL"
        } else {
            ""
        };
        let rows = sqlx::query(&self.render(&format!(
            "SELECT n.id, n.org_id, n.source_resource_type, n.source_resource_id,
                    n.workflow_run_id, n.workflow_node_id, n.channel, n.severity, n.title,
                    n.body, n.target, n.metadata, COALESCE(r.read_at, n.read_at) AS read_at,
                    n.created_at
             FROM notifications n
             LEFT JOIN notification_receipts r ON r.notification_id = n.id AND r.user_id = ?
             WHERE ((n.org_id = ?) OR (n.org_id IS NULL AND ? IS NULL))
               AND r.dismissed_at IS NULL {unread}
             ORDER BY n.created_at DESC LIMIT ?"
        )))
        .bind(user_id)
        .bind(org_id)
        .bind(org_id)
        .bind(bounded_limit)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_notification).collect())
    }

    async fn fetch_notification(
        &self,
        org_id: Option<Uuid>,
        notification_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Notification>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT n.id, n.org_id, n.source_resource_type, n.source_resource_id,
                    n.workflow_run_id, n.workflow_node_id, n.channel, n.severity, n.title,
                    n.body, n.target, n.metadata, COALESCE(r.read_at, n.read_at) AS read_at,
                    n.created_at
             FROM notifications n
             LEFT JOIN notification_receipts r ON r.notification_id = n.id AND r.user_id = ?
             WHERE n.id = ? AND ((n.org_id = ?) OR (n.org_id IS NULL AND ? IS NULL))
               AND r.dismissed_at IS NULL",
        ))
        .bind(user_id)
        .bind(notification_id)
        .bind(org_id)
        .bind(org_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification(&row)))
    }

    async fn mark_notification_read(
        &self,
        org_id: Option<Uuid>,
        notification_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Notification>, SendableError> {
        let exists = sqlx::query(&self.render(
            "SELECT id FROM notifications WHERE id = ? AND ((org_id = ?) OR (org_id IS NULL AND ? IS NULL))",
        ))
        .bind(notification_id)
        .bind(org_id)
        .bind(org_id)
        .fetch_optional(self.pool())
        .await?;
        if exists.is_none() {
            return Ok(None);
        }
        let conflict = self
            .dialect()
            .on_conflict_update("notification_id, user_id", &["read_at"]);
        sqlx::query(&self.render(&format!(
            "INSERT INTO notification_receipts (notification_id, user_id, read_at, dismissed_at)
             VALUES (?, ?, ?, NULL) {conflict}"
        )))
        .bind(notification_id)
        .bind(user_id)
        .bind(Utc::now().timestamp())
        .execute(self.pool())
        .await?;
        let mut rows = self
            .fetch_notifications(org_id, user_id, false, 1000)
            .await?;
        Ok(rows.drain(..).find(|row| row.id == notification_id))
    }

    async fn mark_all_notifications_read(
        &self,
        org_id: Option<Uuid>,
        user_id: Uuid,
    ) -> Result<u64, SendableError> {
        let unread = self
            .fetch_notifications(org_id, user_id, true, 1000)
            .await?;
        let mut count = 0;
        for notification in unread {
            if self
                .mark_notification_read(org_id, notification.id, user_id)
                .await?
                .is_some()
            {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete_notification(
        &self,
        org_id: Option<Uuid>,
        notification_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, SendableError> {
        let exists = sqlx::query(&self.render(
            "SELECT id FROM notifications WHERE id = ? AND ((org_id = ?) OR (org_id IS NULL AND ? IS NULL))",
        ))
        .bind(notification_id)
        .bind(org_id)
        .bind(org_id)
        .fetch_optional(self.pool())
        .await?;
        if exists.is_none() {
            return Ok(false);
        }
        let conflict = self
            .dialect()
            .on_conflict_update("notification_id, user_id", &["dismissed_at"]);
        sqlx::query(&self.render(&format!(
            "INSERT INTO notification_receipts (notification_id, user_id, read_at, dismissed_at)
             VALUES (?, ?, NULL, ?) {conflict}"
        )))
        .bind(notification_id)
        .bind(user_id)
        .bind(Utc::now().timestamp())
        .execute(self.pool())
        .await?;
        Ok(true)
    }

    async fn create_notification_if_absent(
        &self,
        notification: &NewNotification,
    ) -> Result<Option<Notification>, SendableError> {
        let Some(dedupe_key) =
            scoped_dedupe_key(notification.org_id, notification.dedupe_key.as_deref())
        else {
            return self.create_notification(notification).await.map(Some);
        };
        let columns = NOTIFICATION_COLUMNS;
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        let sql = self.dialect().insert_ignore(
            "notifications",
            "id, org_id, source_resource_type, source_resource_id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, dedupe_key, created_at",
            "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?",
            "dedupe_key",
            None,
        );
        let result = sqlx::query(&self.render(&sql))
            .bind(id)
            .bind(notification.org_id)
            .bind(
                notification
                    .source_resource_type
                    .map(|kind| kind.as_str().to_string()),
            )
            .bind(notification.source_resource_id)
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
        org_id: Option<Uuid>,
        workflow_id: Option<Uuid>,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let rows = match workflow_id {
            Some(workflow_id) => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM notification_policies
                     WHERE workflow_id = ? AND ((org_id = ?) OR (org_id IS NULL AND ? IS NULL))
                     ORDER BY created_at DESC"
                )))
                .bind(workflow_id)
                .bind(org_id)
                .bind(org_id)
                .fetch_all(self.pool())
                .await?
            }
            None => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM notification_policies
                     WHERE (org_id = ?) OR (org_id IS NULL AND ? IS NULL)
                     ORDER BY created_at DESC"
                )))
                .bind(org_id)
                .bind(org_id)
                .fetch_all(self.pool())
                .await?
            }
        };
        Ok(rows
            .iter()
            .map(mappers::row_to_notification_policy)
            .collect())
    }

    async fn fetch_notification_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies WHERE id = ?"
        )))
        .bind(policy_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification_policy(&row)))
    }

    async fn fetch_matching_notification_policies(
        &self,
        event: NotificationEvent,
        workflow_id: Uuid,
    ) -> Result<Vec<NotificationPolicy>, SendableError> {
        let columns = NOTIFICATION_POLICY_COLUMNS;
        let enabled = self.dialect().bool_true();
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {columns} FROM notification_policies
             WHERE enabled = {enabled} AND event = ? AND (
                 workflow_id = ? OR (
                     workflow_id IS NULL AND (
                         org_id = (SELECT org_id FROM workflows WHERE id = ?)
                         OR (org_id IS NULL AND (SELECT org_id FROM workflows WHERE id = ?) IS NULL)
                     )
                 )
             )
             ORDER BY created_at",
        )))
        .bind(event.as_str())
        .bind(workflow_id)
        .bind(workflow_id)
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
        let enabled = self.dialect().bool_true();
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
             SET org_id = ?, workflow_id = ?, name = ?, event = ?, severity = ?, channel = ?, target = ?,
                 threshold_seconds = ?, enabled = ?, managed_by = ?, configuration = ?, updated_at = ?
             WHERE id = ?",
        ))
        .bind(policy.org_id)
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
        Ok(retry_delete(|| async {
            sqlx::query(&self.render("DELETE FROM notification_policies WHERE id = ?"))
                .bind(policy_id)
                .execute(self.pool())
                .await
                .map(|result| result.affected() > 0)
        })
        .await?)
    }

    async fn replace_managed_notification_policies(
        &self,
        workflow_id: Uuid,
        managed_by: String,
        policies: Vec<NewNotificationPolicy>,
    ) -> Result<(), SendableError> {
        // drop only this manager's rows so hand-authored policies on the same workflow survive an
        // import, matching how managed triggers reconcile.
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            sqlx::query(&self.render(
                "DELETE FROM notification_policies WHERE workflow_id = ? AND managed_by = ?",
            ))
            .bind(workflow_id)
            .bind(managed_by.as_str())
            .execute(&mut *tx)
            .await?;
            for policy in &policies {
                let now = Utc::now().timestamp();
                sqlx::query(&self.render(&format!(
                    "INSERT INTO notification_policies ({NOTIFICATION_POLICY_COLUMNS})
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )))
                .bind(Uuid::now_v7())
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
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn create_notification_delivery(
        &self,
        delivery_id: Uuid,
        notification_id: Uuid,
        policy_id: Option<Uuid>,
        channel: NotificationChannel,
        target: Option<String>,
        command: runinator_comm::EffectCommand,
    ) -> Result<NotificationDelivery, SendableError> {
        let id = delivery_id;
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO notification_deliveries (id, notification_id, policy_id, channel, target, status, attempts, last_error, dedupe_key, command_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 0, NULL, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(notification_id)
        .bind(policy_id)
        .bind(channel.as_str())
        .bind(target)
        .bind(NotificationDeliveryStatus::Pending.as_str())
        .bind(format!("notification:{id}"))
        .bind(serde_json::to_string(&command)?)
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

    async fn claim_pending_notification_effect_dispatches(
        &self,
        publisher_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<runinator_comm::NotificationEffectDispatchRecord>, SendableError> {
        let columns = "id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until";
        let select = format!(
            "SELECT id FROM notification_deliveries WHERE published_at IS NULL AND command_json IS NOT NULL AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?) ORDER BY created_at, id LIMIT ?{}",
            self.dialect().skip_locked(),
        );
        let ids = sqlx::query(&self.render(&select))
            .bind(now.timestamp())
            .bind(publisher_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        let mut claimed = Vec::with_capacity(ids.len());
        for row in ids {
            let id: Uuid = row.get("id");
            let updated = sqlx::query(&self.render(
                "UPDATE notification_deliveries SET claimed_by = ?, claimed_until = ?, updated_at = ? WHERE id = ? AND published_at IS NULL AND command_json IS NOT NULL AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)",
            ))
            .bind(publisher_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(id)
            .bind(now.timestamp())
            .bind(publisher_id.as_str())
            .execute(self.pool())
            .await?;
            if updated.affected() == 0 {
                continue;
            }
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM notification_deliveries WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(self.pool())
            .await?;
            claimed.push(mappers::row_to_notification_effect_dispatch(&row)?);
        }
        Ok(claimed)
    }

    async fn mark_notification_effect_dispatch_published(
        &self,
        delivery_id: Uuid,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE notification_deliveries SET status = ?, published_at = ?, updated_at = ?, last_error = NULL, claimed_by = NULL, claimed_until = NULL WHERE id = ? AND published_at IS NULL",
        ))
        .bind(NotificationDeliveryStatus::Dispatched.as_str())
        .bind(now)
        .bind(now)
        .bind(delivery_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn mark_notification_effect_dispatch_failed(
        &self,
        delivery_id: Uuid,
        error: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE notification_deliveries SET attempts = attempts + 1, updated_at = ?, last_error = ?, claimed_by = NULL, claimed_until = NULL WHERE id = ? AND published_at IS NULL",
        ))
        .bind(now)
        .bind(error)
        .bind(delivery_id)
        .execute(self.pool())
        .await?;
        Ok(())
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
