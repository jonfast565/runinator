//! approval/gate records and the audit log.
//!
//! the `AutomationStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> AutomationStore for SqlStore<B>
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
    async fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
    ) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE id = ? AND record_type = ?"))
            .bind(record_id)
            .bind(record_type)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_automation_record(&row)))
    }

    async fn fetch_gates(
        &self,
        workflow_run_id: Option<Uuid>,
        status: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, data, created_at, updated_at FROM gates ORDER BY created_at DESC, id DESC",
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_gate)
            .filter(|record| {
                workflow_run_id.is_none_or(|id| {
                    record.get("workflow_run_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                }) && status.as_deref().is_none_or(|status| {
                    record.get("status").and_then(Value::as_str) == Some(status)
                })
            })
            .collect())
    }

    async fn fetch_audit_log(
        &self,
        actor_id: Option<Uuid>,
        action: Option<String>,
        limit: i64,
    ) -> Result<Vec<Value>, SendableError> {
        let mut sql = String::from(
            "SELECT id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at FROM audit_log",
        );
        let mut clauses = Vec::new();
        if actor_id.is_some() {
            clauses.push("actor_id = ?");
        }
        if action.is_some() {
            clauses.push("action = ?");
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ?");
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered);
        if let Some(actor_id) = actor_id {
            query = query.bind(actor_id);
        }
        if let Some(action) = &action {
            query = query.bind(action.clone());
        }
        query = query.bind(limit.max(1));
        let rows = query.fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_audit_log).collect())
    }

    async fn delete_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(
            &self.render("DELETE FROM automation_records WHERE id = ? AND record_type = ?"),
        )
        .bind(record_id)
        .bind(record_type)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn delete_gate(&self, gate_id: Uuid) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("DELETE FROM gates WHERE id = ?"))
            .bind(gate_id)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }
}
