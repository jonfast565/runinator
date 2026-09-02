//! the action outbox, idempotency, and dead letters.
//!
//! the `DeliveryStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;
use runinator_models::ingress_control::{
    BrokerIngressCapture, BrokerIngressCaptureRequest, BrokerIngressRecord, BrokerIngressSession,
    BrokerIngressSessionMode, BrokerMessageDirection, BrokerMessageRecord, IngressControlState,
};

const BROKER_CONTROL_COLUMNS: &str = "id, scope_kind, scope_id, delivery_id, dedupe_key, command_kind, command, state, reviewed_by, last_error, received_at, resolved_at";

fn scope_key(scope: ScopeRef) -> String {
    match scope.id {
        Some(id) => format!("{}:{id}", scope.kind.as_str()),
        None => "platform".into(),
    }
}

fn broker_mode_name(mode: BrokerIngressSessionMode) -> &'static str {
    match mode {
        BrokerIngressSessionMode::Off => "off",
        BrokerIngressSessionMode::Observe => "observe",
        BrokerIngressSessionMode::HoldOrchestrationNudges => "hold_orchestration_nudges",
    }
}

fn broker_state_name(state: IngressControlState) -> &'static str {
    match state {
        IngressControlState::Held => "held",
        IngressControlState::Approved => "approved",
        IngressControlState::Applying => "applying",
        IngressControlState::Applied => "applied",
        IngressControlState::Dropped => "dropped",
        IngressControlState::Failed => "failed",
    }
}

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> DeliveryStore for SqlStore<B>
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
    async fn fetch_broker_ingress_session(
        &self,
        scope: ScopeRef,
    ) -> Result<Option<BrokerIngressSession>, SendableError> {
        let row = sqlx::query(&self.render("SELECT scope_kind, scope_id, mode, updated_by, updated_at, expires_at FROM broker_ingress_sessions WHERE scope_key = ? AND expires_at > ?"))
            .bind(scope_key(scope)).bind(Utc::now().timestamp()).fetch_optional(self.pool()).await?;
        row.as_ref()
            .map(mappers::row_to_broker_ingress_session)
            .transpose()
    }

    async fn put_broker_ingress_session(
        &self,
        session: BrokerIngressSession,
    ) -> Result<BrokerIngressSession, SendableError> {
        let key = scope_key(session.scope);
        if session.mode == BrokerIngressSessionMode::Off {
            sqlx::query(&self.render("DELETE FROM broker_ingress_sessions WHERE scope_key = ?"))
                .bind(key)
                .execute(self.pool())
                .await?;
            return Ok(session);
        }
        let sql = if self.dialect() == SqlDialect::MariaDb {
            "INSERT INTO broker_ingress_sessions (scope_key, scope_kind, scope_id, mode, updated_by, updated_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE mode = VALUES(mode), updated_by = VALUES(updated_by), updated_at = VALUES(updated_at), expires_at = VALUES(expires_at)"
        } else {
            "INSERT INTO broker_ingress_sessions (scope_key, scope_kind, scope_id, mode, updated_by, updated_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(scope_key) DO UPDATE SET mode = excluded.mode, updated_by = excluded.updated_by, updated_at = excluded.updated_at, expires_at = excluded.expires_at"
        };
        sqlx::query(&self.render(sql))
            .bind(key)
            .bind(session.scope.kind.as_str())
            .bind(session.scope.id)
            .bind(broker_mode_name(session.mode))
            .bind(session.updated_by)
            .bind(session.updated_at.timestamp())
            .bind(session.expires_at.timestamp())
            .execute(self.pool())
            .await?;
        self.fetch_broker_ingress_session(session.scope)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("broker ingress session disappeared"))
                    as SendableError
            })
    }

    async fn capture_broker_ingress(
        &self,
        request: BrokerIngressCaptureRequest,
    ) -> Result<BrokerIngressCapture, SendableError> {
        let BrokerIngressCaptureRequest {
            scope,
            delivery_id,
            dedupe_key,
            command_kind,
            command,
            hold,
            received_at,
            capacity,
        } = request;
        let key = scope_key(scope);
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "UPDATE broker_ingress_sessions SET updated_at = updated_at WHERE scope_key = ?",
        ))
        .bind(key.as_str())
        .execute(&mut *tx)
        .await?;
        if let Some(row) = sqlx::query(&self.render(&format!("SELECT {BROKER_CONTROL_COLUMNS} FROM broker_ingress_messages WHERE scope_key = ? AND dedupe_key = ?")))
            .bind(key.as_str()).bind(dedupe_key.as_str()).fetch_optional(&mut *tx).await? {
            let record = mappers::row_to_broker_ingress_record(&row)?;
            tx.commit().await?;
            return Ok(BrokerIngressCapture::Duplicate(record));
        }
        if hold {
            let row = sqlx::query(&self.render("SELECT COUNT(*) AS count FROM broker_ingress_messages WHERE scope_key = ? AND state IN ('held', 'approved')"))
                .bind(key.as_str()).fetch_one(&mut *tx).await?;
            if row.get::<i64, _>("count") >= capacity.max(1) {
                tx.commit().await?;
                return Ok(BrokerIngressCapture::Full);
            }
        }
        let id = Uuid::now_v7();
        let state = if hold { "held" } else { "applying" };
        sqlx::query(&self.render("INSERT INTO broker_ingress_messages (id, scope_key, scope_kind, scope_id, delivery_id, dedupe_key, command_kind, command, state, received_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"))
            .bind(id).bind(key).bind(scope.kind.as_str()).bind(scope.id).bind(delivery_id)
            .bind(dedupe_key).bind(command_kind).bind(command.to_string()).bind(state).bind(received_at.timestamp())
            .execute(&mut *tx).await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {BROKER_CONTROL_COLUMNS} FROM broker_ingress_messages WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let record = mappers::row_to_broker_ingress_record(&row)?;
        tx.commit().await?;
        Ok(if hold {
            BrokerIngressCapture::Held(record)
        } else {
            BrokerIngressCapture::Observed(record)
        })
    }

    async fn fetch_broker_ingress_record(
        &self,
        id: Uuid,
    ) -> Result<Option<BrokerIngressRecord>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {BROKER_CONTROL_COLUMNS} FROM broker_ingress_messages WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref()
            .map(mappers::row_to_broker_ingress_record)
            .transpose()
    }

    async fn fetch_broker_ingress_records(
        &self,
        scope: Option<ScopeRef>,
        state: Option<IngressControlState>,
        limit: i64,
    ) -> Result<Vec<BrokerIngressRecord>, SendableError> {
        let mut sql =
            format!("SELECT {BROKER_CONTROL_COLUMNS} FROM broker_ingress_messages WHERE 1 = 1");
        if scope.is_some() {
            sql.push_str(
                " AND scope_kind = ? AND ((scope_id IS NULL AND ? IS NULL) OR scope_id = ?)",
            );
        }
        if state.is_some() {
            sql.push_str(" AND state = ?");
        }
        sql.push_str(" ORDER BY received_at DESC, id DESC LIMIT ?");
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered);
        if let Some(scope) = scope {
            query = query
                .bind(scope.kind.as_str())
                .bind(scope.id)
                .bind(scope.id);
        }
        if let Some(state) = state {
            query = query.bind(broker_state_name(state));
        }
        let rows = query
            .bind(limit.clamp(1, 1000))
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(mappers::row_to_broker_ingress_record)
            .collect()
    }

    async fn decide_broker_ingress_record(
        &self,
        id: Uuid,
        state: IngressControlState,
        reviewed_by: Uuid,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let terminal = state == IngressControlState::Dropped;
        Ok(sqlx::query(&self.render("UPDATE broker_ingress_messages SET state = ?, reviewed_by = ?, resolved_at = ? WHERE id = ? AND state = 'held'"))
            .bind(broker_state_name(state)).bind(reviewed_by)
            .bind(if terminal { Some(now.timestamp()) } else { None }).bind(id)
            .execute(self.pool()).await?.affected() > 0)
    }

    async fn claim_approved_broker_ingress(
        &self,
        _now: DateTime<Utc>,
    ) -> Result<Option<BrokerIngressRecord>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let lock = match self.dialect() {
            SqlDialect::Postgres | SqlDialect::MariaDb => " FOR UPDATE",
            SqlDialect::Sqlite => "",
        };
        let row = sqlx::query(&self.render(&format!("SELECT id FROM broker_ingress_messages WHERE state = 'approved' ORDER BY received_at, id LIMIT 1{lock}")))
            .fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: Uuid = row.get("id");
        let updated = sqlx::query(&self.render("UPDATE broker_ingress_messages SET state = 'applying' WHERE id = ? AND state = 'approved'"))
            .bind(id).execute(&mut *tx).await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        let row = sqlx::query(&self.render(&format!(
            "SELECT {BROKER_CONTROL_COLUMNS} FROM broker_ingress_messages WHERE id = ?"
        )))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let record = mappers::row_to_broker_ingress_record(&row)?;
        tx.commit().await?;
        Ok(Some(record))
    }

    async fn finish_broker_ingress_record(
        &self,
        id: Uuid,
        state: IngressControlState,
        error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let affected = match error {
            Some(error) => sqlx::query(&self.render("UPDATE broker_ingress_messages SET state = ?, last_error = ?, resolved_at = ? WHERE id = ? AND state = 'applying'"))
                .bind(broker_state_name(state)).bind(error).bind(now.timestamp()).bind(id)
                .execute(self.pool()).await?.affected(),
            None => sqlx::query(&self.render("UPDATE broker_ingress_messages SET state = ?, last_error = NULL, resolved_at = ? WHERE id = ? AND state = 'applying'"))
                .bind(broker_state_name(state)).bind(now.timestamp()).bind(id)
                .execute(self.pool()).await?.affected(),
        };
        Ok(affected > 0)
    }

    async fn purge_broker_ingress_records_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, SendableError> {
        Ok(sqlx::query(&self.render(
            "DELETE FROM broker_ingress_messages WHERE resolved_at IS NOT NULL AND resolved_at < ?",
        ))
        .bind(cutoff.timestamp())
        .execute(self.pool())
        .await?
        .affected())
    }

    async fn record_broker_message(
        &self,
        record: BrokerMessageRecord,
    ) -> Result<(), SendableError> {
        let direction = match record.direction {
            BrokerMessageDirection::Published => "published",
            BrokerMessageDirection::Received => "received",
        };
        sqlx::query(&self.render("INSERT INTO broker_messages (id, channel, direction, message_kind, workflow_run_id, delivery_id, dedupe_key, trace_id, payload, occurred_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"))
            .bind(record.id).bind(record.channel).bind(direction).bind(record.message_kind)
            .bind(record.workflow_run_id).bind(record.delivery_id).bind(record.dedupe_key)
            .bind(record.trace_id).bind(record.payload.to_string()).bind(record.occurred_at.timestamp())
            .execute(self.pool()).await?;
        Ok(())
    }

    async fn fetch_broker_messages(
        &self,
        workflow_run_id: Option<Uuid>,
        pipeline_run_id: Option<Uuid>,
        channel: Option<String>,
        limit: i64,
    ) -> Result<Vec<BrokerMessageRecord>, SendableError> {
        let mut sql = String::from(
            "SELECT id, channel, direction, message_kind, workflow_run_id, delivery_id, dedupe_key, trace_id, payload, occurred_at FROM broker_messages WHERE 1 = 1",
        );
        if workflow_run_id.is_some() {
            sql.push_str(" AND workflow_run_id = ?");
        }
        if pipeline_run_id.is_some() {
            sql.push_str(
                " AND workflow_run_id IN (SELECT id FROM workflow_runs WHERE pipeline_run_id = ?)",
            );
        }
        if channel.is_some() {
            sql.push_str(" AND channel = ?");
        }
        sql.push_str(" ORDER BY occurred_at DESC, id DESC LIMIT ?");
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered);
        if let Some(id) = workflow_run_id {
            query = query.bind(id);
        }
        if let Some(id) = pipeline_run_id {
            query = query.bind(id);
        }
        if let Some(channel) = channel {
            query = query.bind(channel);
        }
        let rows = query
            .bind(limit.clamp(1, 1000))
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(mappers::row_to_broker_message_record)
            .collect()
    }

    async fn purge_broker_messages_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, SendableError> {
        Ok(
            sqlx::query(&self.render("DELETE FROM broker_messages WHERE occurred_at < ?"))
                .bind(cutoff.timestamp())
                .execute(self.pool())
                .await?
                .affected(),
        )
    }

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
        let key_col = self.dialect().ident("key");
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();

        // first writer wins: on conflict keep the existing result rather than overwriting it.
        if self.dialect() == SqlDialect::MariaDb {
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
        let key_col = self.dialect().ident("key");
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
        let key_col = self.dialect().ident("key");
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

        if self.dialect() == SqlDialect::MariaDb {
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
        let key_col = self.dialect().ident("key");
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
        let key_col = self.dialect().ident("key");
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
}
