//! the config/secret store.
//!
//! the `SettingStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> SettingStore for SqlStore<B>
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
    async fn list_stored_settings(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<SettingRecord>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, org_id, kind, scope, name, value, updated_at FROM settings WHERE (org_id = ? OR (org_id IS NULL AND ? IS NULL)) ORDER BY kind, scope, name",
        ))
        .bind(org_id)
        .bind(org_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_setting).collect())
    }

    async fn upsert_setting(
        &self,
        org_id: Option<Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> Result<(), SendableError> {
        if let Some(existing) = self
            .fetch_setting(org_id, kind, scope.clone(), name.clone())
            .await?
        {
            sqlx::query(&self.render("UPDATE settings SET value = ?, updated_at = ? WHERE id = ?"))
                .bind(value)
                .bind(updated_at)
                .bind(existing.id)
                .execute(self.pool())
                .await?;
        } else {
            sqlx::query(&self.render(
                "INSERT INTO settings (id, org_id, kind, scope, name, value, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(Uuid::now_v7())
            .bind(org_id)
            .bind(kind.as_str())
            .bind(scope)
            .bind(name)
            .bind(value)
            .bind(updated_at)
            .execute(self.pool())
            .await?;
        }
        Ok(())
    }

    async fn delete_setting(
        &self,
        org_id: Option<Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            let setting_filter = "id IN (SELECT id FROM settings WHERE (org_id = ? OR (org_id IS NULL AND ? IS NULL)) AND kind = ? AND scope = ? AND name = ?)";
            for table in ["resource_grants", "resource_ownership"] {
                sqlx::query(&self.render(&format!(
                    "DELETE FROM {table} WHERE resource_type = 'setting' AND resource_id IN (SELECT id FROM settings WHERE (org_id = ? OR (org_id IS NULL AND ? IS NULL)) AND kind = ? AND scope = ? AND name = ?)"
                )))
                .bind(org_id)
                .bind(org_id)
                .bind(kind.as_str())
                .bind(scope.as_str())
                .bind(name.as_str())
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query(&self.render(&format!(
                "DELETE FROM settings WHERE {setting_filter}"
            )))
            .bind(org_id)
            .bind(org_id)
            .bind(kind.as_str())
            .bind(scope.as_str())
            .bind(name.as_str())
            .execute(&mut *tx)
            .await?;
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn move_setting(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE settings SET kind = ?, scope = ?, name = ?, updated_at = ? WHERE id = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))",
        ))
        .bind(kind.as_str())
        .bind(scope)
        .bind(name)
        .bind(Utc::now().timestamp())
        .bind(id)
        .bind(org_id)
        .bind(org_id)
        .execute(self.pool())
        .await?;
        if result.affected() == 0 {
            Ok(None)
        } else {
            self.fetch_setting_by_id(org_id, id).await
        }
    }
}
