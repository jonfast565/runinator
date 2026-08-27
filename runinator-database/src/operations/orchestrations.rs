//! Durable correlated orchestration state and reducer outboxes.

use super::*;
use runinator_models::orchestration::{
    AdapterDefinition, AdapterRevision, DeliverySemantics, ExternalOperation,
    ExternalOperationStatus, NewOrchestrationBinding, OrchestrationBinding, OrchestrationCommand,
    OrchestrationEpoch, OrchestrationEventReduction, OrchestrationEvidence,
    OrchestrationPendingIntent, OrchestrationStatus,
};
use runinator_store::roles::{
    ExternalOperationUpdate, NewAdapterDefinition, NewAdapterRevision, NewOrchestrationCommand,
    NewOrchestrationEpoch, OrchestrationBindingUpdate,
};

const BINDING_COLUMNS: &str = "id, admission_id, org_id, scope, correlation_key, generation, pipeline_id, pipeline_revision, pipeline_digest, adapter_id, adapter_revision, policy, status, current_phase, current_attempt, current_epoch, restart_member, resume_existing_epoch, subject_revision, resources, budgets, last_reduced_sequence, version, reducer_lease_owner, reducer_leased_until, created_at, updated_at, finished_at";
const EPOCH_COLUMNS: &str = "id, binding_id, epoch, pipeline_run_id, start_member, parameters, status, reason, created_at, started_at, finished_at";
const REDUCTION_COLUMNS: &str = "id, binding_id, inbox_event_id, sequence, matched_intents, winner, suppressed_intents, binding_version, disposition, detail, created_at";
const PENDING_COLUMNS: &str = "id, binding_id, intent, priority, source_event_ids, latest_payload, wake_at, created_at, updated_at";
const COMMAND_COLUMNS: &str = "id, binding_id, epoch, command_type, operation_key, payload, status, attempts, claimed_by, claimed_until, result, created_at, updated_at";
const EVIDENCE_COLUMNS: &str =
    "id, binding_id, epoch, kind, subject_revision, payload, source_event_id, created_at";
const ADAPTER_COLUMNS: &str = "id, org_id, name, kind, current_revision, enabled, endpoint_identity, has_admitted_binding, created_at, updated_at";
const ADAPTER_REVISION_COLUMNS: &str = "id, adapter_id, revision, kind_version, configuration, secret_bindings, identity_configuration, created_at, actor_id";
const EXTERNAL_OPERATION_COLUMNS: &str = "id, binding_id, epoch, workflow_run_id, effect_id, operation_key, provider, action, semantics, attempt, status, ambiguous, provenance, receipt, created_at, updated_at";

fn external_status(value: ExternalOperationStatus) -> &'static str {
    match value {
        ExternalOperationStatus::Pending => "pending",
        ExternalOperationStatus::Running => "running",
        ExternalOperationStatus::Waiting => "waiting",
        ExternalOperationStatus::Succeeded => "succeeded",
        ExternalOperationStatus::Failed => "failed",
    }
}

fn delivery_semantics(value: DeliverySemantics) -> &'static str {
    match value {
        DeliverySemantics::AtLeastOnce => "at_least_once",
        DeliverySemantics::Idempotent => "idempotent",
        DeliverySemantics::Reconcilable => "reconcilable",
    }
}

impl<B> OrchestrationStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_orchestration_binding(
        &self,
        binding: NewOrchestrationBinding,
    ) -> Result<OrchestrationBinding, SendableError> {
        let now = Utc::now().timestamp();
        let insert = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO orchestration_bindings (id, admission_id, org_id, scope, correlation_key, generation, pipeline_id, pipeline_revision, pipeline_digest, adapter_id, adapter_revision, policy, status, resources, budgets, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', 'null', '{}', ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO orchestration_bindings (id, admission_id, org_id, scope, correlation_key, generation, pipeline_id, pipeline_revision, pipeline_digest, adapter_id, adapter_revision, policy, status, resources, budgets, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', 'null', '{}', ?, ?) ON CONFLICT(admission_id, generation) DO NOTHING"
        };
        sqlx::query(&self.render(insert))
            .bind(binding.id)
            .bind(binding.admission_id)
            .bind(binding.org_id)
            .bind(binding.scope)
            .bind(binding.correlation_key)
            .bind(binding.generation)
            .bind(binding.pipeline_id)
            .bind(binding.pipeline_revision)
            .bind(binding.pipeline_digest)
            .bind(binding.adapter_id)
            .bind(binding.adapter_revision)
            .bind(serde_json::to_string(&binding.policy)?)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
        self.fetch_orchestration_binding_for_admission(binding.admission_id, binding.generation)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "created orchestration binding disappeared",
                )) as SendableError
            })
    }

    async fn fetch_orchestration_binding(
        &self,
        binding_id: Uuid,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {BINDING_COLUMNS} FROM orchestration_bindings WHERE id = ?"
        )))
        .bind(binding_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_orchestration_binding(&row))
            .transpose()
    }

    async fn fetch_orchestration_binding_for_admission(
        &self,
        admission_id: Uuid,
        generation: i64,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        let row = sqlx::query(&self.render(&format!("SELECT {BINDING_COLUMNS} FROM orchestration_bindings WHERE admission_id = ? AND generation = ?")))
            .bind(admission_id).bind(generation).fetch_optional(self.pool()).await?;
        row.map(|row| mappers::row_to_orchestration_binding(&row))
            .transpose()
    }

    async fn fetch_current_orchestration_binding_for_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {BINDING_COLUMNS} FROM orchestration_bindings b \
             INNER JOIN pipeline_runs p ON p.orchestration_binding_id = b.id AND p.execution_epoch = b.current_epoch \
             INNER JOIN workflow_runs w ON w.pipeline_run_id = p.id \
             WHERE w.id = ? AND b.status IN ('pending', 'running', 'waiting', 'suspended')"
        )))
        .bind(workflow_run_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_orchestration_binding(&row))
            .transpose()
    }

    async fn fetch_orchestration_bindings(
        &self,
        org_id: Option<Uuid>,
        status: Option<OrchestrationStatus>,
        limit: i64,
    ) -> Result<Vec<OrchestrationBinding>, SendableError> {
        let mut sql = format!("SELECT {BINDING_COLUMNS} FROM orchestration_bindings WHERE 1 = 1");
        if org_id.is_some() {
            sql.push_str(" AND org_id = ?");
        }
        if status.is_some() {
            sql.push_str(" AND status = ?");
        }
        sql.push_str(" ORDER BY updated_at DESC, id DESC LIMIT ?");
        let rendered = self.render(&sql);
        let mut query = sqlx::query(&rendered);
        if let Some(org_id) = org_id {
            query = query.bind(org_id);
        }
        if let Some(status) = status {
            query = query.bind(status.as_str());
        }
        let rows = query
            .bind(limit.clamp(1, 1000))
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(mappers::row_to_orchestration_binding)
            .collect()
    }

    async fn claim_orchestration_bindings(
        &self,
        owner: String,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<OrchestrationBinding>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {BINDING_COLUMNS} FROM orchestration_bindings WHERE status IN ('pending', 'running', 'waiting', 'suspended') AND (reducer_leased_until IS NULL OR reducer_leased_until < ?) ORDER BY updated_at, id LIMIT ?"
        ))).bind(now.timestamp()).bind(limit.clamp(1, 1000)).fetch_all(self.pool()).await?;
        let mut claimed = Vec::new();
        for row in rows {
            let candidate = mappers::row_to_orchestration_binding(&row)?;
            let updated = sqlx::query(&self.render(
                "UPDATE orchestration_bindings SET reducer_lease_owner = ?, reducer_leased_until = ? WHERE id = ? AND version = ? AND (reducer_leased_until IS NULL OR reducer_leased_until < ?)"
            )).bind(owner.as_str()).bind(leased_until.timestamp()).bind(candidate.id).bind(candidate.version).bind(now.timestamp())
                .execute(self.pool()).await?;
            if updated.affected() > 0
                && let Some(binding) = self.fetch_orchestration_binding(candidate.id).await?
            {
                claimed.push(binding);
            }
        }
        Ok(claimed)
    }

    async fn update_orchestration_binding(
        &self,
        binding_id: Uuid,
        owner: String,
        update: OrchestrationBindingUpdate,
        now: DateTime<Utc>,
    ) -> Result<Option<OrchestrationBinding>, SendableError> {
        let changed = sqlx::query(&self.render(
            "UPDATE orchestration_bindings SET status = ?, current_phase = ?, current_attempt = ?, current_epoch = ?, restart_member = ?, resume_existing_epoch = ?, subject_revision = ?, resources = ?, budgets = ?, last_reduced_sequence = ?, version = version + 1, updated_at = ?, finished_at = ? WHERE id = ? AND version = ? AND reducer_lease_owner = ? AND reducer_leased_until >= ?"
        )).bind(update.status.as_str()).bind(update.current_phase).bind(update.current_attempt)
            .bind(update.current_epoch).bind(update.restart_member).bind(update.resume_existing_epoch)
            .bind(update.subject_revision).bind(update.resources.to_string())
            .bind(serde_json::to_string(&update.budgets)?).bind(update.last_reduced_sequence)
            .bind(now.timestamp()).bind(update.finished_at.map(|value| value.timestamp())).bind(binding_id)
            .bind(update.expected_version).bind(owner).bind(now.timestamp()).execute(self.pool()).await?;
        if changed.affected() == 0 {
            return Ok(None);
        }
        self.fetch_orchestration_binding(binding_id).await
    }

    async fn release_orchestration_binding_lease(
        &self,
        binding_id: Uuid,
        owner: String,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("UPDATE orchestration_bindings SET reducer_lease_owner = NULL, reducer_leased_until = NULL WHERE id = ? AND reducer_lease_owner = ?"))
            .bind(binding_id).bind(owner).execute(self.pool()).await?;
        Ok(result.affected() > 0)
    }

    async fn create_orchestration_epoch(
        &self,
        epoch: NewOrchestrationEpoch,
        now: DateTime<Utc>,
    ) -> Result<OrchestrationEpoch, SendableError> {
        let insert = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO orchestration_epochs (id, binding_id, epoch, start_member, parameters, status, reason, created_at) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO orchestration_epochs (id, binding_id, epoch, start_member, parameters, status, reason, created_at) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?) ON CONFLICT(binding_id, epoch) DO NOTHING"
        };
        sqlx::query(&self.render(insert))
            .bind(epoch.id)
            .bind(epoch.binding_id)
            .bind(epoch.epoch)
            .bind(epoch.start_member)
            .bind(epoch.parameters.to_string())
            .bind(epoch.reason)
            .bind(now.timestamp())
            .execute(self.pool())
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {EPOCH_COLUMNS} FROM orchestration_epochs WHERE binding_id = ? AND epoch = ?"
        )))
        .bind(epoch.binding_id)
        .bind(epoch.epoch)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_orchestration_epoch(&row)
    }

    async fn bind_orchestration_epoch_run(
        &self,
        binding_id: Uuid,
        epoch: i64,
        pipeline_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("UPDATE orchestration_epochs SET pipeline_run_id = ?, status = 'running', started_at = ? WHERE binding_id = ? AND epoch = ? AND pipeline_run_id IS NULL"))
            .bind(pipeline_run_id).bind(now.timestamp()).bind(binding_id).bind(epoch).execute(self.pool()).await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_orchestration_epochs(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<OrchestrationEpoch>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {EPOCH_COLUMNS} FROM orchestration_epochs WHERE binding_id = ? ORDER BY epoch"
        )))
        .bind(binding_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_orchestration_epoch)
            .collect()
    }

    async fn record_orchestration_reduction(
        &self,
        reduction: OrchestrationEventReduction,
    ) -> Result<OrchestrationEventReduction, SendableError> {
        let insert = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO orchestration_event_reductions (id, binding_id, inbox_event_id, sequence, matched_intents, winner, suppressed_intents, binding_version, disposition, detail, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO orchestration_event_reductions (id, binding_id, inbox_event_id, sequence, matched_intents, winner, suppressed_intents, binding_version, disposition, detail, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(inbox_event_id) DO NOTHING"
        };
        sqlx::query(&self.render(insert))
            .bind(reduction.id)
            .bind(reduction.binding_id)
            .bind(reduction.inbox_event_id)
            .bind(reduction.sequence)
            .bind(serde_json::to_string(&reduction.matched_intents)?)
            .bind(reduction.winner)
            .bind(serde_json::to_string(&reduction.suppressed_intents)?)
            .bind(reduction.binding_version)
            .bind(reduction.disposition)
            .bind(reduction.detail.to_string())
            .bind(reduction.created_at.timestamp())
            .execute(self.pool())
            .await?;
        let row = sqlx::query(&self.render(&format!("SELECT {REDUCTION_COLUMNS} FROM orchestration_event_reductions WHERE inbox_event_id = ?")))
            .bind(reduction.inbox_event_id).fetch_one(self.pool()).await?;
        mappers::row_to_orchestration_reduction(&row)
    }

    async fn fetch_orchestration_reductions(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<OrchestrationEventReduction>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {REDUCTION_COLUMNS} FROM orchestration_event_reductions WHERE binding_id = ? ORDER BY sequence")))
            .bind(binding_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_reduction)
            .collect()
    }

    async fn upsert_orchestration_pending_intent(
        &self,
        intent: OrchestrationPendingIntent,
    ) -> Result<OrchestrationPendingIntent, SendableError> {
        let sql = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO orchestration_pending_intents (id, binding_id, intent, priority, source_event_ids, latest_payload, wake_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE priority = VALUES(priority), source_event_ids = VALUES(source_event_ids), latest_payload = VALUES(latest_payload), wake_at = VALUES(wake_at), updated_at = VALUES(updated_at)"
        } else {
            "INSERT INTO orchestration_pending_intents (id, binding_id, intent, priority, source_event_ids, latest_payload, wake_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(binding_id, intent) DO UPDATE SET priority = excluded.priority, source_event_ids = excluded.source_event_ids, latest_payload = excluded.latest_payload, wake_at = excluded.wake_at, updated_at = excluded.updated_at"
        };
        sqlx::query(&self.render(sql))
            .bind(intent.id)
            .bind(intent.binding_id)
            .bind(intent.intent.as_str())
            .bind(i64::from(intent.priority))
            .bind(serde_json::to_string(&intent.source_event_ids)?)
            .bind(intent.latest_payload.to_string())
            .bind(intent.wake_at.timestamp())
            .bind(intent.created_at.timestamp())
            .bind(intent.updated_at.timestamp())
            .execute(self.pool())
            .await?;
        let row = sqlx::query(&self.render(&format!("SELECT {PENDING_COLUMNS} FROM orchestration_pending_intents WHERE binding_id = ? AND intent = ?")))
            .bind(intent.binding_id).bind(intent.intent).fetch_one(self.pool()).await?;
        mappers::row_to_orchestration_pending_intent(&row)
    }

    async fn fetch_due_orchestration_intents(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<OrchestrationPendingIntent>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {PENDING_COLUMNS} FROM orchestration_pending_intents WHERE wake_at <= ? ORDER BY wake_at, id LIMIT ?")))
            .bind(now.timestamp()).bind(limit.clamp(1, 1000)).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_pending_intent)
            .collect()
    }

    async fn fetch_orchestration_pending_intents(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<OrchestrationPendingIntent>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {PENDING_COLUMNS} FROM orchestration_pending_intents WHERE binding_id = ? ORDER BY priority DESC, wake_at")))
            .bind(binding_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_pending_intent)
            .collect()
    }

    async fn delete_orchestration_pending_intents_below(
        &self,
        binding_id: Uuid,
        priority: i32,
    ) -> Result<u64, SendableError> {
        let result = sqlx::query(&self.render(
            "DELETE FROM orchestration_pending_intents WHERE binding_id = ? AND priority < ?",
        ))
        .bind(binding_id)
        .bind(i64::from(priority))
        .execute(self.pool())
        .await?;
        Ok(result.affected())
    }

    async fn delete_orchestration_pending_intent(
        &self,
        binding_id: Uuid,
        intent: String,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "DELETE FROM orchestration_pending_intents WHERE binding_id = ? AND intent = ?",
        ))
        .bind(binding_id)
        .bind(intent)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn enqueue_orchestration_command(
        &self,
        command: NewOrchestrationCommand,
        now: DateTime<Utc>,
    ) -> Result<OrchestrationCommand, SendableError> {
        let insert = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO orchestration_commands (id, binding_id, epoch, command_type, operation_key, payload, status, result, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', 'null', ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO orchestration_commands (id, binding_id, epoch, command_type, operation_key, payload, status, result, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', 'null', ?, ?) ON CONFLICT(binding_id, operation_key) DO NOTHING"
        };
        sqlx::query(&self.render(insert))
            .bind(command.id)
            .bind(command.binding_id)
            .bind(command.epoch)
            .bind(command.command_type)
            .bind(command.operation_key.as_str())
            .bind(command.payload.to_string())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .execute(self.pool())
            .await?;
        let row = sqlx::query(&self.render(&format!("SELECT {COMMAND_COLUMNS} FROM orchestration_commands WHERE binding_id = ? AND operation_key = ?")))
            .bind(command.binding_id).bind(command.operation_key).fetch_one(self.pool()).await?;
        mappers::row_to_orchestration_command(&row)
    }

    async fn fetch_orchestration_commands(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<OrchestrationCommand>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {COMMAND_COLUMNS} FROM orchestration_commands WHERE binding_id = ? ORDER BY created_at, id")))
            .bind(binding_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_command)
            .collect()
    }

    async fn claim_orchestration_commands(
        &self,
        owner: String,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<OrchestrationCommand>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {COMMAND_COLUMNS} FROM orchestration_commands WHERE status = 'pending' OR (status = 'claimed' AND claimed_until < ?) ORDER BY created_at, id LIMIT ?")))
            .bind(now.timestamp()).bind(limit.clamp(1, 1000)).fetch_all(self.pool()).await?;
        let mut claimed = Vec::new();
        for row in rows {
            let command = mappers::row_to_orchestration_command(&row)?;
            let result = sqlx::query(&self.render("UPDATE orchestration_commands SET status = 'claimed', attempts = attempts + 1, claimed_by = ?, claimed_until = ?, updated_at = ? WHERE id = ? AND (status = 'pending' OR (status = 'claimed' AND claimed_until < ?))"))
                .bind(owner.as_str()).bind(leased_until.timestamp()).bind(now.timestamp()).bind(command.id).bind(now.timestamp())
                .execute(self.pool()).await?;
            if result.affected() > 0 {
                let row = sqlx::query(&self.render(&format!(
                    "SELECT {COMMAND_COLUMNS} FROM orchestration_commands WHERE id = ?"
                )))
                .bind(command.id)
                .fetch_one(self.pool())
                .await?;
                claimed.push(mappers::row_to_orchestration_command(&row)?);
            }
        }
        Ok(claimed)
    }

    async fn complete_orchestration_command(
        &self,
        command_id: Uuid,
        owner: String,
        succeeded: bool,
        result: Value,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let status = if succeeded { "succeeded" } else { "failed" };
        let changed = sqlx::query(&self.render("UPDATE orchestration_commands SET status = ?, result = ?, claimed_until = NULL, updated_at = ? WHERE id = ? AND status = 'claimed' AND claimed_by = ?"))
            .bind(status).bind(result.to_string()).bind(now.timestamp()).bind(command_id).bind(owner)
            .execute(self.pool()).await?;
        Ok(changed.affected() > 0)
    }

    async fn append_orchestration_evidence(
        &self,
        evidence: OrchestrationEvidence,
    ) -> Result<(), SendableError> {
        let insert = match self.dialect() {
            SqlDialect::MySql => {
                "INSERT IGNORE INTO orchestration_evidence (id, binding_id, epoch, kind, subject_revision, payload, source_event_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
            }
            _ => {
                "INSERT INTO orchestration_evidence (id, binding_id, epoch, kind, subject_revision, payload, source_event_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO NOTHING"
            }
        };
        sqlx::query(&self.render(insert))
            .bind(evidence.id)
            .bind(evidence.binding_id)
            .bind(evidence.epoch)
            .bind(evidence.kind)
            .bind(evidence.subject_revision)
            .bind(evidence.payload.to_string())
            .bind(evidence.source_event_id)
            .bind(evidence.created_at.timestamp())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn fetch_orchestration_evidence(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<OrchestrationEvidence>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {EVIDENCE_COLUMNS} FROM orchestration_evidence WHERE binding_id = ? ORDER BY created_at, id")))
            .bind(binding_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_evidence)
            .collect()
    }

    async fn create_orchestration_adapter(
        &self,
        adapter: NewAdapterDefinition,
        now: DateTime<Utc>,
    ) -> Result<(AdapterDefinition, AdapterRevision), SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("INSERT INTO orchestration_adapters (id, org_id, name, kind, current_revision, enabled, endpoint_identity, has_admitted_binding, created_at, updated_at) VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?)"))
            .bind(adapter.id).bind(adapter.org_id).bind(adapter.name).bind(adapter.kind)
            .bind(true).bind(adapter.endpoint_identity).bind(false).bind(now.timestamp()).bind(now.timestamp())
            .execute(&mut *tx).await?;
        sqlx::query(&self.render("INSERT INTO orchestration_adapter_revisions (id, adapter_id, revision, kind_version, configuration, secret_bindings, identity_configuration, created_at, actor_id) VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?)"))
            .bind(Uuid::now_v7()).bind(adapter.id).bind(adapter.kind_version)
            .bind(adapter.configuration.to_string()).bind(serde_json::to_string(&adapter.secret_bindings)?)
            .bind(adapter.identity_configuration.to_string()).bind(now.timestamp()).bind(adapter.actor_id)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        let definition = self
            .fetch_orchestration_adapter(adapter.id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("created adapter disappeared")) as SendableError
            })?;
        let revision = self
            .fetch_orchestration_adapter_revision(adapter.id, 1)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "created adapter revision disappeared",
                )) as SendableError
            })?;
        Ok((definition, revision))
    }

    async fn fetch_orchestration_adapter(
        &self,
        adapter_id: Uuid,
    ) -> Result<Option<AdapterDefinition>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {ADAPTER_COLUMNS} FROM orchestration_adapters WHERE id = ?"
        )))
        .bind(adapter_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_orchestration_adapter(&row))
            .transpose()
    }

    async fn fetch_orchestration_adapter_by_endpoint(
        &self,
        endpoint_identity: String,
    ) -> Result<Option<AdapterDefinition>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {ADAPTER_COLUMNS} FROM orchestration_adapters WHERE endpoint_identity = ?"
        )))
        .bind(endpoint_identity)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_orchestration_adapter(&row))
            .transpose()
    }

    async fn fetch_orchestration_adapters(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<AdapterDefinition>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {ADAPTER_COLUMNS} FROM orchestration_adapters WHERE org_id = ? ORDER BY name, id")))
            .bind(org_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_adapter)
            .collect()
    }

    async fn fetch_orchestration_adapter_revision(
        &self,
        adapter_id: Uuid,
        revision: i64,
    ) -> Result<Option<AdapterRevision>, SendableError> {
        let row = sqlx::query(&self.render(&format!("SELECT {ADAPTER_REVISION_COLUMNS} FROM orchestration_adapter_revisions WHERE adapter_id = ? AND revision = ?")))
            .bind(adapter_id).bind(revision).fetch_optional(self.pool()).await?;
        row.map(|row| mappers::row_to_orchestration_adapter_revision(&row))
            .transpose()
    }

    async fn fetch_orchestration_adapter_revisions(
        &self,
        adapter_id: Uuid,
    ) -> Result<Vec<AdapterRevision>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {ADAPTER_REVISION_COLUMNS} FROM orchestration_adapter_revisions WHERE adapter_id = ? ORDER BY revision DESC")))
            .bind(adapter_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_orchestration_adapter_revision)
            .collect()
    }

    async fn create_orchestration_adapter_revision(
        &self,
        revision: NewAdapterRevision,
        now: DateTime<Utc>,
    ) -> Result<Option<(AdapterDefinition, AdapterRevision)>, SendableError> {
        let Some(existing) = self
            .fetch_orchestration_adapter(revision.adapter_id)
            .await?
        else {
            return Ok(None);
        };
        if existing.current_revision != revision.expected_revision {
            return Ok(None);
        }
        let current = self
            .fetch_orchestration_adapter_revision(revision.adapter_id, revision.expected_revision)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("current adapter revision is missing"))
                    as SendableError
            })?;
        if existing.has_admitted_binding
            && current.identity_configuration != revision.identity_configuration
        {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "adapter identity extraction is immutable after its first admitted binding",
            )) as SendableError);
        }
        let next = revision.expected_revision + 1;
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render("UPDATE orchestration_adapters SET current_revision = ?, updated_at = ? WHERE id = ? AND current_revision = ?"))
            .bind(next).bind(now.timestamp()).bind(revision.adapter_id).bind(revision.expected_revision)
            .execute(&mut *tx).await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query(&self.render("INSERT INTO orchestration_adapter_revisions (id, adapter_id, revision, kind_version, configuration, secret_bindings, identity_configuration, created_at, actor_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"))
            .bind(revision.id).bind(revision.adapter_id).bind(next).bind(revision.kind_version)
            .bind(revision.configuration.to_string()).bind(serde_json::to_string(&revision.secret_bindings)?)
            .bind(revision.identity_configuration.to_string()).bind(now.timestamp()).bind(revision.actor_id)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        let definition = self
            .fetch_orchestration_adapter(revision.adapter_id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other("updated adapter disappeared")) as SendableError
            })?;
        let revision = self
            .fetch_orchestration_adapter_revision(revision.adapter_id, next)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "updated adapter revision disappeared",
                )) as SendableError
            })?;
        Ok(Some((definition, revision)))
    }

    async fn set_orchestration_adapter_enabled(
        &self,
        adapter_id: Uuid,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<Option<AdapterDefinition>, SendableError> {
        let changed =
            sqlx::query(&self.render(
                "UPDATE orchestration_adapters SET enabled = ?, updated_at = ? WHERE id = ?",
            ))
            .bind(enabled)
            .bind(now.timestamp())
            .bind(adapter_id)
            .execute(self.pool())
            .await?;
        if changed.affected() == 0 {
            return Ok(None);
        }
        self.fetch_orchestration_adapter(adapter_id).await
    }

    async fn mark_orchestration_adapter_admitted(
        &self,
        adapter_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render("UPDATE orchestration_adapters SET has_admitted_binding = ?, updated_at = ? WHERE id = ? AND has_admitted_binding = ?"))
            .bind(true).bind(now.timestamp()).bind(adapter_id).bind(false).execute(self.pool()).await?;
        Ok(changed.affected() > 0)
    }

    async fn delete_orchestration_adapter(&self, adapter_id: Uuid) -> Result<bool, SendableError> {
        let changed = sqlx::query(&self.render(
            "DELETE FROM orchestration_adapters WHERE id = ? AND has_admitted_binding = ?",
        ))
        .bind(adapter_id)
        .bind(false)
        .execute(self.pool())
        .await?;
        Ok(changed.affected() > 0)
    }

    async fn create_external_operation(
        &self,
        operation: ExternalOperation,
    ) -> Result<ExternalOperation, SendableError> {
        let insert = if self.dialect() == SqlDialect::MySql {
            "INSERT INTO external_operations (id, binding_id, epoch, workflow_run_id, effect_id, operation_key, provider, action, semantics, attempt, status, ambiguous, provenance, receipt, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE id = id"
        } else {
            "INSERT INTO external_operations (id, binding_id, epoch, workflow_run_id, effect_id, operation_key, provider, action, semantics, attempt, status, ambiguous, provenance, receipt, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(binding_id, operation_key) DO NOTHING"
        };
        sqlx::query(&self.render(insert))
            .bind(operation.id)
            .bind(operation.binding_id)
            .bind(operation.epoch)
            .bind(operation.workflow_run_id)
            .bind(operation.effect_id)
            .bind(operation.operation_key.as_str())
            .bind(operation.provider)
            .bind(operation.action)
            .bind(delivery_semantics(operation.semantics))
            .bind(operation.attempt)
            .bind(external_status(operation.status))
            .bind(operation.ambiguous)
            .bind(operation.provenance.to_string())
            .bind(operation.receipt.to_string())
            .bind(operation.created_at.timestamp())
            .bind(operation.updated_at.timestamp())
            .execute(self.pool())
            .await?;
        let row = sqlx::query(&self.render(&format!("SELECT {EXTERNAL_OPERATION_COLUMNS} FROM external_operations WHERE binding_id = ? AND operation_key = ?")))
            .bind(operation.binding_id).bind(operation.operation_key).fetch_one(self.pool()).await?;
        mappers::row_to_external_operation(&row)
    }

    async fn fetch_external_operation(
        &self,
        operation_id: Uuid,
    ) -> Result<Option<ExternalOperation>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {EXTERNAL_OPERATION_COLUMNS} FROM external_operations WHERE id = ?"
        )))
        .bind(operation_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_external_operation(&row))
            .transpose()
    }

    async fn fetch_external_operation_for_effect(
        &self,
        effect_id: Uuid,
    ) -> Result<Option<ExternalOperation>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {EXTERNAL_OPERATION_COLUMNS} FROM external_operations WHERE effect_id = ?"
        )))
        .bind(effect_id)
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| mappers::row_to_external_operation(&row))
            .transpose()
    }

    async fn fetch_external_operations(
        &self,
        binding_id: Uuid,
    ) -> Result<Vec<ExternalOperation>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {EXTERNAL_OPERATION_COLUMNS} FROM external_operations WHERE binding_id = ? ORDER BY created_at, id")))
            .bind(binding_id).fetch_all(self.pool()).await?;
        rows.iter()
            .map(mappers::row_to_external_operation)
            .collect()
    }

    async fn update_external_operation(
        &self,
        operation_id: Uuid,
        update: ExternalOperationUpdate,
        now: DateTime<Utc>,
    ) -> Result<Option<ExternalOperation>, SendableError> {
        let changed = sqlx::query(&self.render("UPDATE external_operations SET status = ?, attempt = ?, ambiguous = ?, provenance = ?, receipt = ?, updated_at = ? WHERE id = ?"))
            .bind(external_status(update.status)).bind(update.attempt).bind(update.ambiguous)
            .bind(update.provenance.to_string()).bind(update.receipt.to_string()).bind(now.timestamp()).bind(operation_id)
            .execute(self.pool()).await?;
        if changed.affected() == 0 {
            return Ok(None);
        }
        self.fetch_external_operation(operation_id).await
    }
}
