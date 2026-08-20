//! Transactional persistence for compiled workflow modules, continuations, and durable effects.

use super::*;
use runinator_comm::EffectCommand;
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect, WorkflowEffectStatus,
    WorkflowJournalEntry, WorkflowJournalRecord, WorkflowModule,
};

const CONTINUATION_COLUMNS: &str = "id, workflow_run_id, module_version, continuation_json, status, version, ready_at, claimed_by, claimed_until, created_at, updated_at";
const EFFECT_COLUMNS: &str = "id, version, workflow_run_id, continuation_id, sequence, attempt, request_json, status, result_json, message, created_at, updated_at, finished_at";
const JOURNAL_COLUMNS: &str =
    "id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at";

fn wire_name<T: serde::Serialize>(value: &T) -> Result<String, SendableError> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_string())
}

fn cas_error() -> SendableError {
    crate::errors::WORKFLOW_VM_CORRUPT_STATE
        .error("stale workflow continuation revision; reload before driving it again")
}

impl<B> WorkflowVmStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Vec<u8>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
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
    async fn fetch_workflow_module(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowModule>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT version, module_json FROM workflow_vm_modules WHERE workflow_run_id = ?",
        ))
        .bind(workflow_run_id)
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else { return Ok(None) };
        let module: WorkflowModule =
            serde_json::from_str(&row.try_get::<String, _>("module_json")?)
                .map_err(|error| crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(error))?;
        let version = row.try_get::<i64, _>("version")? as u32;
        if module.version != version || !module.is_supported() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("workflow module row version does not match a supported payload"));
        }
        Ok(Some(module))
    }

    async fn fetch_workflow_continuation(
        &self,
        continuation_id: Uuid,
    ) -> Result<Option<WorkflowContinuation>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE id = ?"
        )))
        .bind(continuation_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref()
            .map(mappers::row_to_workflow_continuation)
            .transpose()
    }

    async fn fetch_workflow_effect(
        &self,
        effect_id: Uuid,
    ) -> Result<Option<WorkflowEffect>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {EFFECT_COLUMNS} FROM workflow_effects WHERE id = ?"
        )))
        .bind(effect_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref()
            .map(mappers::row_to_workflow_effect)
            .transpose()
    }

    async fn fetch_workflow_journal(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowJournalRecord>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {JOURNAL_COLUMNS} FROM workflow_journal_entries WHERE workflow_run_id = ? ORDER BY sequence"
        )))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_workflow_journal_record)
            .collect()
    }

    async fn create_workflow_vm(
        &self,
        module: WorkflowModule,
        continuation: WorkflowContinuation,
    ) -> Result<(), SendableError> {
        if !module.is_supported()
            || !continuation.is_supported()
            || continuation.module_version != module.version
        {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot create a workflow VM from incompatible module state"));
        }
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_vm_modules (workflow_run_id, version, module_json, created_at) VALUES (?, ?, ?, ?)",
        ))
        .bind(continuation.workflow_run_id)
        .bind(i64::from(module.version))
        .bind(serde_json::to_string(&module)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_continuations (id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(continuation.id)
        .bind(continuation.workflow_run_id)
        .bind(i64::from(continuation.module_version))
        .bind(serde_json::to_string(&continuation)?)
        .bind(wire_name(&continuation.status)?)
        .bind(continuation.revision as i64)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let entry = WorkflowJournalEntry::Entered {
            continuation_id: continuation.id,
            instruction_pointer: continuation.instruction_pointer,
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_EFFECT_PROTOCOL_VERSION))
        .bind(continuation.workflow_run_id)
        .bind(0_i64)
        .bind(Some(continuation.id))
        .bind(Option::<Uuid>::None)
        .bind(serde_json::to_string(&entry)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn suspend_on_effect(
        &self,
        mut continuation: WorkflowContinuation,
        effect: WorkflowEffect,
        command: EffectCommand,
    ) -> Result<WorkflowEffect, SendableError> {
        if !effect.is_supported()
            || !command.is_supported()
            || effect.continuation_id != continuation.id
            || effect.workflow_run_id != continuation.workflow_run_id
            || command.effect_id != effect.id
            || command.continuation_id != continuation.id
            || continuation.awaiting_effect_id != Some(effect.id)
        {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot persist an incompatible workflow effect protocol"));
        }
        let mut tx = self.pool().begin().await?;
        let existing = sqlx::query(&self.render(&format!(
            "SELECT {EFFECT_COLUMNS} FROM workflow_effects WHERE continuation_id = ? AND sequence = ?"
        )))
        .bind(effect.continuation_id)
        .bind(effect.sequence as i64)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            let existing = mappers::row_to_workflow_effect(&row)?;
            tx.commit().await?;
            return Ok(existing);
        }
        let expected = continuation.revision;
        continuation.revision += 1;
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = NULL, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&continuation)?)
        .bind(wire_name(&continuation.status)?)
        .bind(continuation.revision as i64)
        .bind(effect.updated_at)
        .bind(continuation.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        sqlx::query(&self.render(
            "INSERT INTO workflow_effects (id, version, workflow_run_id, continuation_id, sequence, attempt, request_json, status, result_json, message, idempotency_key, created_at, updated_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(effect.id)
        .bind(i64::from(effect.version))
        .bind(effect.workflow_run_id)
        .bind(effect.continuation_id)
        .bind(effect.sequence as i64)
        .bind(i64::from(effect.attempt))
        .bind(serde_json::to_string(&effect.request)?)
        .bind(wire_name(&effect.status)?)
        .bind(effect.result.as_ref().map(serde_json::to_string).transpose()?)
        .bind(effect.message.clone())
        .bind(effect.idempotency_key())
        .bind(effect.created_at)
        .bind(effect.updated_at)
        .bind(effect.finished_at)
        .execute(&mut *tx)
        .await?;
        let lock = match self.dialect() {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE",
        };
        sqlx::query(&self.render(&format!("SELECT id FROM workflow_runs WHERE id = ?{lock}")))
            .bind(effect.workflow_run_id)
            .fetch_one(&mut *tx)
            .await?;
        let sequence: i64 = sqlx::query_scalar(&self.render(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM workflow_journal_entries WHERE workflow_run_id = ?",
        ))
        .bind(effect.workflow_run_id)
        .fetch_one(&mut *tx)
        .await?;
        let entry = WorkflowJournalEntry::EffectRequested {
            effect_id: effect.id,
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_EFFECT_PROTOCOL_VERSION))
        .bind(effect.workflow_run_id)
        .bind(sequence)
        .bind(Some(effect.continuation_id))
        .bind(Some(effect.id))
        .bind(serde_json::to_string(&entry)?)
        .bind(effect.created_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_effect_dispatches (id, effect_id, dedupe_key, command_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(command.command_id)
        .bind(effect.id)
        .bind(effect.idempotency_key())
        .bind(serde_json::to_string(&command)?)
        .bind(effect.created_at)
        .bind(effect.updated_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(effect)
    }

    async fn fork_workflow_continuation(
        &self,
        mut parent: WorkflowContinuation,
        children: Vec<WorkflowContinuation>,
        join_key: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let expected = parent.revision;
        parent.revision += 1;
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = NULL, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&parent)?)
        .bind(wire_name(&parent.status)?)
        .bind(parent.revision as i64)
        .bind(now)
        .bind(parent.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        for child in &children {
            sqlx::query(&self.render(
                "INSERT INTO workflow_continuations (id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(child.id)
            .bind(child.workflow_run_id)
            .bind(i64::from(child.module_version))
            .bind(serde_json::to_string(child)?)
            .bind(wire_name(&child.status)?)
            .bind(child.revision as i64)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        let lock = match self.dialect() {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE",
        };
        sqlx::query(&self.render(&format!("SELECT id FROM workflow_runs WHERE id = ?{lock}")))
            .bind(parent.workflow_run_id)
            .fetch_one(&mut *tx)
            .await?;
        let sequence: i64 = sqlx::query_scalar(&self.render(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM workflow_journal_entries WHERE workflow_run_id = ?",
        ))
        .bind(parent.workflow_run_id)
        .fetch_one(&mut *tx)
        .await?;
        let entry = WorkflowJournalEntry::Forked {
            continuation_id: parent.id,
            children: children.iter().map(|child| child.id).collect(),
            join_key,
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_EFFECT_PROTOCOL_VERSION))
        .bind(parent.workflow_run_id)
        .bind(sequence)
        .bind(Some(parent.id))
        .bind(Option::<Uuid>::None)
        .bind(serde_json::to_string(&entry)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn settle_workflow_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        status: WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
        settled_at: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        if !status.is_terminal() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("an effect may only be settled to a terminal status"));
        }
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {EFFECT_COLUMNS} FROM workflow_effects WHERE id = ?"
        )))
        .bind(effect_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(false);
        };
        let effect = mappers::row_to_workflow_effect(&row)?;
        if effect.attempt != attempt || effect.status.is_terminal() {
            tx.commit().await?;
            return Ok(false);
        }
        let now = settled_at.timestamp();
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_effects SET status = ?, result_json = ?, message = ?, updated_at = ?, finished_at = ? WHERE id = ? AND attempt = ? AND status IN ('requested', 'running')",
        ))
        .bind(wire_name(&status)?)
        .bind(output.as_ref().map(serde_json::to_string).transpose()?)
        .bind(message.clone())
        .bind(now)
        .bind(now)
        .bind(effect_id)
        .bind(i64::from(attempt))
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }
        sqlx::query(&self.render(
            "UPDATE workflow_continuations SET status = 'runnable', ready_at = ?, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND status = 'waiting'",
        ))
        .bind(now)
        .bind(now)
        .bind(effect.continuation_id)
        .execute(&mut *tx)
        .await?;
        let lock = match self.dialect() {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE",
        };
        sqlx::query(&self.render(&format!("SELECT id FROM workflow_runs WHERE id = ?{lock}")))
            .bind(effect.workflow_run_id)
            .fetch_one(&mut *tx)
            .await?;
        let sequence: i64 = sqlx::query_scalar(&self.render(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM workflow_journal_entries WHERE workflow_run_id = ?",
        ))
        .bind(effect.workflow_run_id)
        .fetch_one(&mut *tx)
        .await?;
        let entry = WorkflowJournalEntry::EffectSettled { effect_id, status };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_EFFECT_PROTOCOL_VERSION))
        .bind(effect.workflow_run_id)
        .bind(sequence)
        .bind(Some(effect.continuation_id))
        .bind(Some(effect_id))
        .bind(serde_json::to_string(&entry)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn claim_runnable_workflow_continuations(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowContinuation>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let ids = sqlx::query(&self.render(&format!(
            "SELECT id FROM workflow_continuations WHERE status = 'runnable' AND ready_at <= ? AND (claimed_until IS NULL OR claimed_until <= ?) ORDER BY ready_at, id LIMIT ?{}",
            self.dialect().skip_locked()
        )))
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(limit.max(1))
        .fetch_all(&mut *tx)
        .await?;
        let ids = ids
            .iter()
            .map(|row| row.try_get::<Uuid, _>("id"))
            .collect::<Result<Vec<_>, _>>()?;
        let mut claimed = Vec::with_capacity(ids.len());
        for id in ids {
            sqlx::query(&self.render(
                "UPDATE workflow_continuations SET claimed_by = ?, claimed_until = ? WHERE id = ?",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(id)
            .execute(&mut *tx)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            claimed.push(mappers::row_to_workflow_continuation(&row)?);
        }
        tx.commit().await?;
        Ok(claimed)
    }
}
