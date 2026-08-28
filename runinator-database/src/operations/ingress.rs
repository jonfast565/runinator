//! Durable generic ingress admission.

use super::*;
use runinator_models::orchestration::{
    IngressAdmission, IngressAdmissionClaim, IngressEvent, IngressEventDisposition,
    IngressEventRecord, IngressPromotion, IngressQueueState,
};

const INGRESS_ADMISSION_COLUMNS: &str = "id, org_id, scope, correlation_key, generation, target_kind, target_id, status, workflow_run_id, pipeline_run_id, policy, created_at, updated_at";
const INGRESS_EVENT_COLUMNS: &str = "id, admission_id, sequence, generation, source, event_id, event_type, correlation_key, payload, provenance, occurred_at, received_at, disposition, queue_state, claim_token, promoted_generation, workflow_run_id, pipeline_run_id";

fn disposition_name(value: IngressEventDisposition) -> &'static str {
    match value {
        IngressEventDisposition::Started => "started",
        IngressEventDisposition::Recorded => "recorded",
        IngressEventDisposition::Queued => "queued",
        IngressEventDisposition::InterruptRequested => "interrupt_requested",
        IngressEventDisposition::Requeued => "requeued",
        IngressEventDisposition::Rejected => "rejected",
    }
}

macro_rules! settle_and_promote {
    ($store:expr, $run_column:literal, $run_id:expr, $now:expr) => {{
        let mut tx = $store.pool().begin().await?;
        let lock = match $store.dialect() {
            SqlDialect::Postgres | SqlDialect::MariaDb => " FOR UPDATE",
            SqlDialect::Sqlite => "",
        };
        let admission_sql = format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE {} = ? AND status = 'active'{}",
            $run_column, lock
        );
        let row = sqlx::query(&$store.render(&admission_sql))
            .bind($run_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let admission = mappers::row_to_ingress_admission(&row)?;
        let admission_id = admission.id.expect("stored ingress admission id");
        sqlx::query(&$store.render(
            "UPDATE ingress_admissions SET status = 'terminal', updated_at = ? WHERE id = ? AND status = 'active'",
        ))
        .bind($now.timestamp()).bind(admission_id).execute(&mut *tx).await?;

        let event_sql = format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? AND queue_state = 'queued' ORDER BY sequence LIMIT 1{lock}"
        );
        let event_row = sqlx::query(&$store.render(&event_sql))
            .bind(admission_id).fetch_optional(&mut *tx).await?;
        let Some(event_row) = event_row else {
            tx.commit().await?;
            return Ok(None);
        };
        let mut event = mappers::row_to_ingress_event(&event_row)?;
        let claim_token = Uuid::now_v7();
        let next_generation = admission.generation + 1;
        let claimed = sqlx::query(&$store.render(
            "UPDATE ingress_events SET queue_state = 'claimed', claim_token = ?, promoted_generation = ? WHERE id = ? AND queue_state = 'queued'",
        )).bind(claim_token).bind(next_generation).bind(event.id)
        .execute(&mut *tx).await?;
        if claimed.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query(&$store.render(
            "UPDATE ingress_admissions SET generation = ?, status = 'active', workflow_run_id = NULL, pipeline_run_id = NULL, updated_at = ? WHERE id = ? AND status = 'terminal'",
        )).bind(next_generation).bind($now.timestamp()).bind(admission_id)
        .execute(&mut *tx).await?;
        let updated_row = sqlx::query(&$store.render(&format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE id = ?"
        ))).bind(admission_id).fetch_one(&mut *tx).await?;
        let updated = mappers::row_to_ingress_admission(&updated_row)?;
        event.queue_state = IngressQueueState::Claimed;
        event.promoted_generation = Some(next_generation);
        event.queue_position = Some(1);
        tx.commit().await?;
        Ok(Some(IngressPromotion { admission: updated, event, claim_token }))
    }};
}

impl<B> IngressStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn claim_ingress_admission(
        &self,
        admission: IngressAdmission,
        initial_event: Option<IngressEvent>,
    ) -> Result<IngressAdmissionClaim, SendableError> {
        let id = admission.id.unwrap_or_else(Uuid::now_v7);
        let org_scope = admission
            .org_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        let target_kind = match admission.target.kind {
            runinator_models::orchestration::IngressTargetKind::Workflow => "workflow",
            runinator_models::orchestration::IngressTargetKind::Pipeline => "pipeline",
        };
        let status = match admission.status {
            runinator_models::orchestration::IngressAdmissionStatus::Active => "active",
            runinator_models::orchestration::IngressAdmissionStatus::Terminal => "terminal",
        };
        let sql = if self.dialect() == SqlDialect::MariaDb {
            "INSERT INTO ingress_admissions (id, org_scope, org_id, scope, correlation_key, generation, target_kind, target_id, status, workflow_run_id, pipeline_run_id, policy, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO ingress_admissions (id, org_scope, org_id, scope, correlation_key, generation, target_kind, target_id, status, workflow_run_id, pipeline_run_id, policy, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(org_scope, scope, correlation_key) DO NOTHING"
        };
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(sql))
            .bind(id)
            .bind(org_scope.clone())
            .bind(admission.org_id)
            .bind(admission.scope.as_str())
            .bind(admission.correlation_key.as_str())
            .bind(admission.generation)
            .bind(target_kind)
            .bind(admission.target.id)
            .bind(status)
            .bind(admission.workflow_run_id)
            .bind(admission.pipeline_run_id)
            .bind(admission.policy.to_string())
            .bind(admission.created_at.timestamp())
            .bind(admission.updated_at.timestamp())
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE org_scope = ? AND scope = ? AND correlation_key = ?"
        )))
        .bind(org_scope)
        .bind(admission.scope)
        .bind(admission.correlation_key)
        .fetch_one(&mut *tx)
        .await?;
        let saved = mappers::row_to_ingress_admission(&row)?;
        let claim = if saved.id == Some(id) {
            if let Some(event) = initial_event {
                sqlx::query(&self.render(
                    "INSERT INTO ingress_events (id, admission_id, sequence, generation, source, event_id, event_type, correlation_key, payload, provenance, occurred_at, received_at, disposition, queue_state) VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'started', 'none')",
                )).bind(Uuid::now_v7()).bind(id).bind(saved.generation)
                .bind(event.source).bind(event.event_id).bind(event.event_type).bind(event.correlation_key)
                .bind(event.payload.to_string()).bind(event.provenance.to_string())
                .bind(event.occurred_at.map(|value| value.timestamp()))
                .bind(saved.created_at.timestamp()).execute(&mut *tx).await?;
            }
            IngressAdmissionClaim::Acquired(saved)
        } else {
            IngressAdmissionClaim::Existing(saved)
        };
        tx.commit().await?;
        Ok(claim)
    }

    async fn fetch_ingress_admission(
        &self,
        org_id: Option<Uuid>,
        scope: String,
        correlation_key: String,
    ) -> Result<Option<IngressAdmission>, SendableError> {
        let org_scope = org_id.map(|id| id.to_string()).unwrap_or_default();
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE org_scope = ? AND scope = ? AND correlation_key = ?"
        )))
        .bind(org_scope)
        .bind(scope)
        .bind(correlation_key)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_ingress_admission(&row))
            .transpose()
    }

    async fn record_ingress_event(
        &self,
        admission_id: Uuid,
        generation: i64,
        event: IngressEvent,
        disposition: IngressEventDisposition,
        queued: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<IngressEventRecord, SendableError> {
        let id = Uuid::now_v7();
        let queue_state = if queued { "queued" } else { "none" };
        let mut tx = self.pool().begin().await?;
        // A harmless write locks the sole admission owner on SQLite and its row on server
        // backends. Sequence assignment and deduplication are therefore one serialized decision.
        sqlx::query(
            &self.render("UPDATE ingress_admissions SET updated_at = updated_at WHERE id = ?"),
        )
        .bind(admission_id)
        .execute(&mut *tx)
        .await?;
        let existing = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? AND source = ? AND event_id = ?"
        ))).bind(admission_id).bind(event.source.as_str()).bind(event.event_id.as_str())
        .fetch_optional(&mut *tx).await?;
        if let Some(row) = existing {
            let mut entry = mappers::row_to_ingress_event(&row)?;
            if entry.queue_state == IngressQueueState::Queued {
                let row = sqlx::query(&self.render(
                    "SELECT COUNT(*) AS count FROM ingress_events WHERE admission_id = ? AND queue_state IN ('queued', 'claimed') AND sequence <= ?",
                )).bind(admission_id).bind(entry.sequence).fetch_one(&mut *tx).await?;
                entry.queue_position = Some(row.get::<i64, _>("count"));
            }
            tx.commit().await?;
            return Ok(IngressEventRecord {
                entry,
                duplicate: true,
            });
        }
        let row = sqlx::query(&self.render(
            "SELECT COALESCE(MAX(sequence), 0) AS max_sequence FROM ingress_events WHERE admission_id = ?",
        )).bind(admission_id).fetch_one(&mut *tx).await?;
        let sequence = row.get::<i64, _>("max_sequence") + 1;
        sqlx::query(&self.render(
            "INSERT INTO ingress_events (id, admission_id, sequence, generation, source, event_id, event_type, correlation_key, payload, provenance, occurred_at, received_at, disposition, queue_state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
            .bind(id).bind(admission_id).bind(sequence).bind(generation)
            .bind(event.source.as_str()).bind(event.event_id.as_str()).bind(event.event_type.as_str())
            .bind(event.correlation_key.as_str()).bind(event.payload.to_string())
            .bind(event.provenance.to_string())
            .bind(event.occurred_at.map(|value| value.timestamp())).bind(now.timestamp())
            .bind(disposition_name(disposition)).bind(queue_state)
            .execute(&mut *tx).await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? AND source = ? AND event_id = ?"
        )))
        .bind(admission_id).bind(event.source).bind(event.event_id)
        .fetch_one(&mut *tx).await?;
        let mut entry = mappers::row_to_ingress_event(&row)?;
        if entry.queue_state == IngressQueueState::Queued {
            let row = sqlx::query(&self.render(
                "SELECT COUNT(*) AS count FROM ingress_events WHERE admission_id = ? AND queue_state IN ('queued', 'claimed') AND sequence <= ?",
            )).bind(admission_id).bind(entry.sequence).fetch_one(&mut *tx).await?;
            entry.queue_position = Some(row.get::<i64, _>("count"));
        }
        tx.commit().await?;
        Ok(IngressEventRecord {
            entry,
            duplicate: false,
        })
    }

    async fn fetch_ingress_events(
        &self,
        admission_id: Uuid,
    ) -> Result<Vec<runinator_models::orchestration::IngressInboxEntry>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? ORDER BY sequence"
        ))).bind(admission_id).fetch_all(self.pool()).await?;
        rows.iter().map(mappers::row_to_ingress_event).collect()
    }

    async fn fetch_ingress_event(
        &self,
        admission_id: Uuid,
        source: String,
        event_id: String,
    ) -> Result<Option<runinator_models::orchestration::IngressInboxEntry>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? AND source = ? AND event_id = ?"
        ))).bind(admission_id).bind(source).bind(event_id).fetch_optional(self.pool()).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut entry = mappers::row_to_ingress_event(&row)?;
        if matches!(
            entry.queue_state,
            IngressQueueState::Queued | IngressQueueState::Claimed
        ) {
            let row = sqlx::query(&self.render(
                "SELECT COUNT(*) AS count FROM ingress_events WHERE admission_id = ? AND queue_state IN ('queued', 'claimed') AND sequence <= ?",
            )).bind(admission_id).bind(entry.sequence).fetch_one(self.pool()).await?;
            entry.queue_position = Some(row.get::<i64, _>("count"));
        }
        Ok(Some(entry))
    }

    async fn bind_ingress_event_result(
        &self,
        event_id: Uuid,
        workflow_run_id: Option<Uuid>,
        pipeline_run_id: Option<Uuid>,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "UPDATE ingress_events SET workflow_run_id = ?, pipeline_run_id = ?, queue_state = CASE WHEN queue_state = 'claimed' THEN 'promoted' ELSE queue_state END WHERE id = ?",
        )).bind(workflow_run_id).bind(pipeline_run_id).bind(event_id)
        .execute(self.pool()).await?.affected() > 0)
    }

    async fn bind_ingress_workflow_run(
        &self,
        admission_id: Uuid,
        workflow_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "UPDATE ingress_admissions SET workflow_run_id = ?, updated_at = ? WHERE id = ? AND target_kind = 'workflow' AND status = 'active' AND workflow_run_id IS NULL",
        ))
        .bind(workflow_run_id)
        .bind(now.timestamp())
        .bind(admission_id)
        .execute(self.pool())
        .await?
        .affected()
            > 0)
    }

    async fn bind_ingress_pipeline_run(
        &self,
        admission_id: Uuid,
        pipeline_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "UPDATE ingress_admissions SET pipeline_run_id = ?, updated_at = ? WHERE id = ? AND target_kind = 'pipeline' AND status = 'active' AND pipeline_run_id IS NULL",
        ))
        .bind(pipeline_run_id)
        .bind(now.timestamp())
        .bind(admission_id)
        .execute(self.pool())
        .await?
        .affected()
            > 0)
    }

    async fn settle_ingress_workflow_run(
        &self,
        workflow_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "UPDATE ingress_admissions SET status = 'terminal', updated_at = ? WHERE workflow_run_id = ? AND status = 'active'",
        ))
        .bind(now.timestamp())
        .bind(workflow_run_id)
        .execute(self.pool())
        .await?
        .affected()
            > 0)
    }

    async fn settle_ingress_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "UPDATE ingress_admissions SET status = 'terminal', updated_at = ? WHERE pipeline_run_id = ? AND status = 'active'",
        ))
        .bind(now.timestamp())
        .bind(pipeline_run_id)
        .execute(self.pool())
        .await?
        .affected()
            > 0)
    }

    async fn settle_ingress_admission(
        &self,
        admission_id: Uuid,
        generation: i64,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE ingress_admissions SET status = 'terminal', updated_at = ? WHERE id = ? AND generation = ? AND status = 'active'",
        ))
        .bind(now.timestamp())
        .bind(admission_id)
        .bind(generation)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn settle_and_promote_ingress_workflow_run(
        &self,
        workflow_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<IngressPromotion>, SendableError> {
        settle_and_promote!(self, "workflow_run_id", workflow_run_id, now)
    }

    async fn settle_and_promote_ingress_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<IngressPromotion>, SendableError> {
        settle_and_promote!(self, "pipeline_run_id", pipeline_run_id, now)
    }

    async fn release_ingress_promotion(
        &self,
        claim_token: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(&self.render(
            "SELECT admission_id, promoted_generation FROM ingress_events WHERE claim_token = ? AND queue_state = 'claimed'",
        )).bind(claim_token).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(false);
        };
        let admission_id: Uuid = row.get("admission_id");
        let Some(promoted_generation) = row.get::<Option<i64>, _>("promoted_generation") else {
            tx.rollback().await?;
            return Ok(false);
        };
        sqlx::query(&self.render(
            "UPDATE ingress_events SET queue_state = 'queued', claim_token = NULL, promoted_generation = NULL WHERE claim_token = ? AND queue_state = 'claimed'",
        )).bind(claim_token).execute(&mut *tx).await?;
        let result = sqlx::query(&self.render(
            "UPDATE ingress_admissions SET generation = generation - 1, status = 'terminal', workflow_run_id = NULL, pipeline_run_id = NULL, updated_at = ? WHERE id = ? AND generation = ? AND workflow_run_id IS NULL AND pipeline_run_id IS NULL",
        )).bind(now.timestamp()).bind(admission_id).bind(promoted_generation)
        .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.affected() > 0)
    }

    async fn claim_queued_ingress_event(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<IngressPromotion>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let lock = match self.dialect() {
            SqlDialect::Postgres | SqlDialect::MariaDb => " FOR UPDATE",
            SqlDialect::Sqlite => "",
        };
        let row = sqlx::query(&self.render(&format!(
            "SELECT e.id AS event_row_id, e.admission_id AS admission_row_id FROM ingress_events e JOIN ingress_admissions a ON a.id = e.admission_id WHERE e.queue_state = 'queued' AND a.status = 'terminal' ORDER BY e.received_at, e.sequence LIMIT 1{lock}"
        ))).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let event_id: Uuid = row.get("event_row_id");
        let admission_id: Uuid = row.get("admission_row_id");
        let admission_row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE id = ?"
        )))
        .bind(admission_id)
        .fetch_one(&mut *tx)
        .await?;
        let admission = mappers::row_to_ingress_admission(&admission_row)?;
        let next_generation = admission.generation + 1;
        let claim_token = Uuid::now_v7();
        let claimed = sqlx::query(&self.render(
            "UPDATE ingress_events SET queue_state = 'claimed', claim_token = ?, promoted_generation = ? WHERE id = ? AND queue_state = 'queued'",
        )).bind(claim_token).bind(next_generation).bind(event_id).execute(&mut *tx).await?;
        if claimed.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        let promoted = sqlx::query(&self.render(
            "UPDATE ingress_admissions SET generation = ?, status = 'active', workflow_run_id = NULL, pipeline_run_id = NULL, updated_at = ? WHERE id = ? AND status = 'terminal'",
        )).bind(next_generation).bind(now.timestamp()).bind(admission_id).execute(&mut *tx).await?;
        if promoted.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        let admission_row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_ADMISSION_COLUMNS} FROM ingress_admissions WHERE id = ?"
        )))
        .bind(admission_id)
        .fetch_one(&mut *tx)
        .await?;
        let event_row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE id = ?"
        )))
        .bind(event_id)
        .fetch_one(&mut *tx)
        .await?;
        let admission = mappers::row_to_ingress_admission(&admission_row)?;
        let mut event = mappers::row_to_ingress_event(&event_row)?;
        event.queue_position = Some(1);
        tx.commit().await?;
        Ok(Some(IngressPromotion {
            admission,
            event,
            claim_token,
        }))
    }

    async fn release_unbound_ingress_admission(
        &self,
        admission_id: Uuid,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render(
            "DELETE FROM ingress_admissions WHERE id = ? AND status = 'active' AND workflow_run_id IS NULL AND pipeline_run_id IS NULL",
        ))
        .bind(admission_id)
        .execute(self.pool())
        .await?
        .affected()
            > 0)
    }

    async fn requeue_ingress_event(
        &self,
        admission_id: Uuid,
        expected_generation: i64,
        target: runinator_models::orchestration::IngressTarget,
        policy: Value,
        event: IngressEvent,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<IngressEventRecord>, SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            &self.render("UPDATE ingress_admissions SET updated_at = updated_at WHERE id = ?"),
        )
        .bind(admission_id)
        .execute(&mut *tx)
        .await?;
        let existing = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE admission_id = ? AND source = ? AND event_id = ?"
        )))
        .bind(admission_id)
        .bind(event.source.as_str())
        .bind(event.event_id.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            let entry = mappers::row_to_ingress_event(&row)?;
            tx.commit().await?;
            return Ok(Some(IngressEventRecord {
                entry,
                duplicate: true,
            }));
        }
        let row = sqlx::query(
            &self.render("SELECT generation, status FROM ingress_admissions WHERE id = ?"),
        )
        .bind(admission_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        if row.get::<i64, _>("generation") != expected_generation
            || row.get::<String, _>("status") != "terminal"
        {
            tx.commit().await?;
            return Ok(None);
        }
        let row = sqlx::query(&self.render(
            "SELECT COALESCE(MAX(sequence), 0) AS max_sequence FROM ingress_events WHERE admission_id = ?",
        ))
        .bind(admission_id)
        .fetch_one(&mut *tx)
        .await?;
        let sequence = row.get::<i64, _>("max_sequence") + 1;
        let next_generation = expected_generation + 1;
        let entry_id = Uuid::now_v7();
        sqlx::query(&self.render(
            "INSERT INTO ingress_events (id, admission_id, sequence, generation, source, event_id, event_type, correlation_key, payload, provenance, occurred_at, received_at, disposition, queue_state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'requeued', 'none')",
        ))
        .bind(entry_id).bind(admission_id).bind(sequence).bind(next_generation)
        .bind(event.source).bind(event.event_id).bind(event.event_type).bind(event.correlation_key)
        .bind(event.payload.to_string()).bind(event.provenance.to_string())
        .bind(event.occurred_at.map(|value| value.timestamp()))
        .bind(now.timestamp()).execute(&mut *tx).await?;
        let target_kind = match target.kind {
            runinator_models::orchestration::IngressTargetKind::Workflow => "workflow",
            runinator_models::orchestration::IngressTargetKind::Pipeline => "pipeline",
        };
        let updated = sqlx::query(&self.render(
            "UPDATE ingress_admissions SET generation = ?, target_kind = ?, target_id = ?, status = 'active', workflow_run_id = NULL, pipeline_run_id = NULL, policy = ?, updated_at = ? WHERE id = ? AND generation = ? AND status = 'terminal'",
        )).bind(next_generation).bind(target_kind).bind(target.id).bind(policy.to_string())
        .bind(now.timestamp()).bind(admission_id).bind(expected_generation)
        .execute(&mut *tx).await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        let row = sqlx::query(&self.render(&format!(
            "SELECT {INGRESS_EVENT_COLUMNS} FROM ingress_events WHERE id = ?"
        )))
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await?;
        let entry = mappers::row_to_ingress_event(&row)?;
        tx.commit().await?;
        Ok(Some(IngressEventRecord {
            entry,
            duplicate: false,
        }))
    }
}
