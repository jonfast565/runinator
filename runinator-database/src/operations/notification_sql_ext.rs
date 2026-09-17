#[allow(unused_imports)]
use super::*;

pub(super) trait NotificationSqlExt: SqlBackend {
    async fn insert_notification_policy(
        &self,
        id: Uuid,
        policy: &NewNotificationPolicy,
    ) -> Result<(), SendableError>;
}

impl<B> NotificationSqlExt for B
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    <B::Db as Database>::Arguments: IntoArguments<B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    async fn insert_notification_policy(
        &self,
        id: Uuid,
        policy: &NewNotificationPolicy,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(&format!(
            "INSERT INTO notification_policies ({NOTIFICATION_POLICY_COLUMNS})
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(id)
        .bind(policy.org_id)
        .bind(policy.workflow_id)
        .bind(policy.name.as_str())
        .bind(policy.event.as_str())
        .bind(policy.severity.as_str())
        .bind(policy.channel.as_str())
        .bind(policy.provider.clone())
        .bind(policy.function.clone())
        .bind(policy.interactive)
        .bind(policy.target.clone())
        .bind(policy.threshold_seconds)
        .bind(policy.enabled)
        .bind(policy.managed_by.clone())
        .bind(policy.configuration.to_string())
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
