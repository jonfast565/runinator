//! the action outbox, idempotency, and dead letters.
//!
//! the `DispatchStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> DispatchStore for SqlStore<B>
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
    async fn record_dead_letter(&self, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let payload = record
            .get("payload")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        sqlx::query(&self.render(
            "INSERT INTO dead_letters (id, channel, event_id, dedupe_key, attempts, error, payload, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(json_str(&record, "channel"))
        .bind(json_opt_uuid(&record, "event_id"))
        .bind(json_opt_str(&record, "dedupe_key"))
        .bind(json_opt_i64(&record, "attempts").unwrap_or(0))
        .bind(json_str(&record, "error"))
        .bind(payload.to_string())
        .bind(now)
        .execute(self.pool())
        .await?;
        let row = sqlx::query(&self.render(
            "SELECT id, channel, event_id, dedupe_key, attempts, error, payload, created_at FROM dead_letters WHERE id = ?",
        ))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_dead_letter(&row))
    }

    async fn fetch_dead_letters(
        &self,
        channel: Option<String>,
        limit: i64,
    ) -> Result<Vec<Value>, SendableError> {
        let mut sql = String::from(
            "SELECT id, channel, event_id, dedupe_key, attempts, error, payload, created_at FROM dead_letters",
        );
        if channel.is_some() {
            sql.push_str(" WHERE channel = ?");
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ?");
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered);
        if let Some(channel) = &channel {
            query = query.bind(channel.clone());
        }
        query = query.bind(limit.max(1));
        let rows = query.fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_dead_letter).collect())
    }

    async fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> Result<Value, SendableError> {
        // `key` is reserved in mysql; quote it for every dialect via ident.
        let key_col = queries::ident(self.dialect(), "key");
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();

        // first writer wins: on conflict keep the existing result rather than overwriting it.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(&format!(
                "INSERT INTO idempotency_keys (id, scope, {key_col}, result, created_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON DUPLICATE KEY UPDATE result = result",
            )))
            .bind(id)
            .bind(scope.as_str())
            .bind(key.as_str())
            .bind(result.to_string())
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT id, scope, {key_col}, result, created_at FROM idempotency_keys WHERE scope = ? AND {key_col} = ?",
            )))
            .bind(scope)
            .bind(key)
            .fetch_one(self.pool())
            .await?;
            return Ok(mappers::row_to_idempotency_key(&row));
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO idempotency_keys (id, scope, {key_col}, result, created_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(scope, {key_col}) DO UPDATE SET result = idempotency_keys.result
             RETURNING id, scope, {key_col}, result, created_at",
        )))
        .bind(id)
        .bind(scope)
        .bind(key)
        .bind(result.to_string())
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_idempotency_key(&row))
    }

    async fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> Result<Option<Value>, SendableError> {
        let key_col = queries::ident(self.dialect(), "key");
        let row = sqlx::query(&self.render(&format!("SELECT id, scope, {key_col}, result, created_at FROM idempotency_keys WHERE scope = ? AND {key_col} = ?")))
            .bind(scope)
            .bind(key)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_idempotency_key(&row)))
    }

    async fn claim_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> Result<IdempotencyClaim, SendableError> {
        let key_col = queries::ident(self.dialect(), "key");
        let id = Uuid::now_v7();
        let ts = now.timestamp();
        let stale_ts = stale_before.timestamp();
        // one upsert decides the whole thing, so two concurrent claimants cannot both acquire. the
        // owner moves to us only when the row is unfinished *and* takeable: never claimed, left by the
        // manual put/get store, already ours, or abandoned (a reservation older than `stale_before`,
        // which is how a crashed worker's claim stops blocking the key forever). a completed row keeps
        // its owner so we read it back as a replayable result instead of taking it over.
        let claim_case = "CASE
               WHEN idempotency_keys.completed_at IS NOT NULL THEN idempotency_keys.owner_node_run_id
               WHEN idempotency_keys.owner_node_run_id IS NULL
                 OR idempotency_keys.owner_node_run_id = ?
                 OR idempotency_keys.claimed_at IS NULL
                 OR idempotency_keys.claimed_at < ? THEN excluded.owner_node_run_id
               ELSE idempotency_keys.owner_node_run_id
             END";

        if self.dialect() == SqlDialect::MySql {
            // mysql has no RETURNING. the read-back is still safe: once this statement leaves the row
            // owned by us no other claimant can move it, and if we lost, the only state the winner can
            // reach meanwhile is `completed` — which is a better answer for us, not a wrong one.
            let mysql_case =
                claim_case.replace("excluded.owner_node_run_id", "VALUES(owner_node_run_id)");
            sqlx::query(&self.render(&format!(
                "INSERT INTO idempotency_keys (id, scope, {key_col}, result, created_at, owner_node_run_id, claimed_at)
                 VALUES (?, ?, ?, '{{}}', ?, ?, ?)
                 ON DUPLICATE KEY UPDATE
                   claimed_at = CASE WHEN owner_node_run_id = {mysql_case} THEN VALUES(claimed_at) ELSE claimed_at END,
                   owner_node_run_id = {mysql_case}",
            )))
            .bind(id)
            .bind(scope.as_str())
            .bind(key.as_str())
            .bind(ts)
            .bind(owner_node_run_id)
            .bind(ts)
            .bind(owner_node_run_id)
            .bind(stale_ts)
            .bind(owner_node_run_id)
            .bind(stale_ts)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT owner_node_run_id, completed_at, result FROM idempotency_keys
                 WHERE scope = ? AND {key_col} = ?",
            )))
            .bind(scope)
            .bind(key)
            .fetch_one(self.pool())
            .await?;
            return Ok(row_to_idempotency_claim(&row, owner_node_run_id));
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO idempotency_keys (id, scope, {key_col}, result, created_at, owner_node_run_id, claimed_at)
             VALUES (?, ?, ?, '{{}}', ?, ?, ?)
             ON CONFLICT(scope, {key_col}) DO UPDATE SET
               claimed_at = CASE WHEN {claim_case} = excluded.owner_node_run_id
                 THEN excluded.claimed_at ELSE idempotency_keys.claimed_at END,
               owner_node_run_id = {claim_case}
             RETURNING owner_node_run_id, completed_at, result",
        )))
        .bind(id)
        .bind(scope)
        .bind(key)
        .bind(ts)
        .bind(owner_node_run_id)
        .bind(ts)
        .bind(owner_node_run_id)
        .bind(stale_ts)
        .bind(owner_node_run_id)
        .bind(stale_ts)
        .fetch_one(self.pool())
        .await?;
        Ok(row_to_idempotency_claim(&row, owner_node_run_id))
    }

    async fn complete_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
        result: Value,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let key_col = queries::ident(self.dialect(), "key");
        // conditional on still owning an unfinished reservation, so a superseded claimant writing
        // late cannot overwrite the winner's recorded result. first completion wins.
        let updated = self
            .pool()
            .execute(
                sqlx::query(&self.render(&format!(
                    "UPDATE idempotency_keys SET result = ?, completed_at = ?
                     WHERE scope = ? AND {key_col} = ?
                       AND owner_node_run_id = ? AND completed_at IS NULL",
                )))
                .bind(result.to_string())
                .bind(now.timestamp())
                .bind(scope)
                .bind(key)
                .bind(owner_node_run_id),
            )
            .await?;
        Ok(updated.affected() > 0)
    }

    async fn release_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
    ) -> Result<bool, SendableError> {
        let key_col = queries::ident(self.dialect(), "key");
        // free our own unfinished reservation so a retry or a later run is not held off for the whole
        // staleness window. conditional on ownership and on still being unfinished, so this can never
        // clear a completed result or another claimant's live reservation.
        let updated = self
            .pool()
            .execute(
                sqlx::query(&self.render(&format!(
                    "UPDATE idempotency_keys SET owner_node_run_id = NULL, claimed_at = NULL
                     WHERE scope = ? AND {key_col} = ?
                       AND owner_node_run_id = ? AND completed_at IS NULL",
                )))
                .bind(scope)
                .bind(key)
                .bind(owner_node_run_id),
            )
            .await?;
        Ok(updated.affected() > 0)
    }

    async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until
             FROM workflow_action_dispatches
             WHERE published_at IS NULL
             ORDER BY updated_at ASC, id ASC
             LIMIT ?",
        ))
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_action_dispatch).collect()
    }

    async fn claim_pending_action_dispatches(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        let columns = "id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until";

        // mysql has no UPDATE ... RETURNING and cannot subquery the table being updated, so claim
        // via a derived-table subselect, then read the claimed rows back by the lease just written.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE workflow_action_dispatches
                 SET claimed_by = ?, claimed_until = ?, updated_at = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_action_dispatches
                         WHERE published_at IS NULL
                           AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                         ORDER BY updated_at ASC, id ASC
                         LIMIT ?
                     ) AS claimable
                 )",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_action_dispatches WHERE claimed_by = ? AND claimed_until = ? ORDER BY updated_at ASC, id ASC",
            )))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(mappers::row_to_action_dispatch).collect();
        }

        let sql = self.render(&format!(
            "UPDATE workflow_action_dispatches
             SET claimed_by = ?, claimed_until = ?, updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_action_dispatches
                 WHERE published_at IS NULL
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY updated_at ASC, id ASC
                 LIMIT ?{skip}
             )
             RETURNING {columns}",
            skip = queries::skip_locked(self.dialect()),
        ));
        let rows = sqlx::query(&sql)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(mappers::row_to_action_dispatch).collect()
    }

    async fn mark_action_dispatch_published(&self, dispatch_id: Uuid) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE workflow_action_dispatches
             SET published_at = ?, updated_at = ?, last_error = NULL, claimed_by = NULL, claimed_until = NULL
             WHERE id = ?",
        ))
        .bind(now)
        .bind(now)
        .bind(dispatch_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: Uuid,
        error: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE workflow_action_dispatches
             SET attempts = attempts + 1, updated_at = ?, last_error = ?, claimed_by = NULL, claimed_until = NULL
             WHERE id = ?",
        ))
        .bind(now)
        .bind(error)
        .bind(dispatch_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
