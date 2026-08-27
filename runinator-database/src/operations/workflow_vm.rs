//! Transactional persistence for compiled workflow modules, continuations, and durable effects.

use super::*;
use runinator_comm::{EffectCommand, EffectDispatchRecord};
use runinator_models::interrupt::InterruptSource;
use runinator_models::workflow_vm::{
    WORKFLOW_JOURNAL_VERSION, WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffect,
    WorkflowEffectOutputEvent, WorkflowEffectStatus, WorkflowFrame, WorkflowInterruptOutcome,
    WorkflowJournalEntry, WorkflowJournalRecord, WorkflowModule, WorkflowPendingInterrupt,
};
use runinator_store::roles::{NewWorkflowVmRun, WorkflowTimerInterrupt};

const CONTINUATION_COLUMNS: &str = "id, workflow_run_id, module_version, continuation_json, status, version, ready_at, claimed_by, claimed_until, created_at, updated_at";
const EFFECT_COLUMNS: &str = "id, version, workflow_run_id, continuation_id, sequence, attempt, request_json, status, current_executor_replica_id, last_executor_replica_id, result_json, message, created_at, updated_at, finished_at";
const JOURNAL_COLUMNS: &str =
    "id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at";

fn wire_name<T: serde::Serialize>(value: &T) -> Result<String, SendableError> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_string())
}

/// Release every named mutex held by one run. Called from the same transaction that settles or
/// cancels the run, so a terminal run can never leave a key locked behind it.
async fn release_run_mutexes<B>(
    store: &SqlStore<B>,
    tx: &mut sqlx::Transaction<'_, B::Db>,
    workflow_run_id: Uuid,
    now: i64,
) -> Result<(), SendableError>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    sqlx::query(&store.render(
        "UPDATE workflow_mutexes SET holder_run_id = NULL, holder_continuation_id = NULL, acquired_at = NULL, hold_deadline = NULL, overdue_at = NULL, updated_at = ? WHERE holder_run_id = ?",
    ))
    .bind(now)
    .bind(workflow_run_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Append one journal record inside an open transaction, allocating its per-run sequence.
async fn append_journal<B>(
    store: &SqlStore<B>,
    tx: &mut sqlx::Transaction<'_, B::Db>,
    workflow_run_id: Uuid,
    continuation_id: Option<Uuid>,
    entry: &WorkflowJournalEntry,
    now: i64,
) -> Result<(), SendableError>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    let lock = match store.dialect() {
        SqlDialect::Sqlite => "",
        SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE",
    };
    sqlx::query(&store.render(&format!("SELECT id FROM workflow_runs WHERE id = ?{lock}")))
        .bind(workflow_run_id)
        .fetch_one(&mut **tx)
        .await?;
    let sequence: i64 = sqlx::query(&store.render(
        "SELECT COALESCE(MAX(sequence), -1) + 1 AS next_sequence FROM workflow_journal_entries WHERE workflow_run_id = ?",
    ))
    .bind(workflow_run_id)
    .fetch_one(&mut **tx)
    .await?
    .try_get("next_sequence")?;
    sqlx::query(&store.render(
        "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
    ))
    .bind(Uuid::now_v7())
    .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
    .bind(workflow_run_id)
    .bind(sequence)
    .bind(continuation_id)
    .bind(serde_json::to_string(entry)?)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Persist all graph nodes an interpreter drive entered before reaching its next durable boundary.
/// Keeping the short-lived queue on the continuation makes the trace crash-safe without turning
/// pure bytecode instructions into store calls.
async fn append_node_entries<B>(
    store: &SqlStore<B>,
    tx: &mut sqlx::Transaction<'_, B::Db>,
    workflow_run_id: Uuid,
    continuation_id: Uuid,
    node_ids: Vec<String>,
    now: i64,
) -> Result<(), SendableError>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    for node_id in node_ids {
        append_journal(
            store,
            tx,
            workflow_run_id,
            Some(continuation_id),
            &WorkflowJournalEntry::NodeEntered {
                continuation_id,
                node_id,
            },
            now,
        )
        .await?;
    }
    Ok(())
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
    async fn create_workflow_vm_run(
        &self,
        start: NewWorkflowVmRun,
    ) -> Result<WorkflowRun, SendableError> {
        if !start.module.is_supported() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot create a workflow run from an incompatible module"));
        }

        let NewWorkflowVmRun {
            workflow_id,
            workflow_snapshot,
            parameters,
            config,
            state,
            name,
            provenance,
            pipeline_run_id,
            pipeline_member_attempt_id,
            module,
            instruction_pointer,
        } = start;
        if instruction_pointer >= module.instructions.len() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(format!(
                "initial workflow instruction pointer {instruction_pointer} is outside a {}-instruction module",
                module.instructions.len()
            )));
        }
        let run_id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        let state = WorkflowExecutionState::from_state(&state);
        let mut continuation = WorkflowContinuation::start(run_id, module.version);
        continuation.instruction_pointer = instruction_pointer;
        continuation
            .locals
            .insert("input".into(), parameters.clone());
        continuation.locals.insert("config".into(), config);
        let entry = WorkflowJournalEntry::Entered {
            continuation_id: continuation.id,
            instruction_pointer: continuation.instruction_pointer,
        };

        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, pipeline_run_id, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, '{}', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(run_id)
        .bind(workflow_id)
        .bind(serde_json::to_string(&workflow_snapshot)?)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(now)
        .bind(name)
        .bind(pipeline_run_id)
        .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_replica_id)
        .bind(provenance.actor_display_name)
        .bind(provenance.request_host)
        .bind(provenance.request_ip)
        .bind(provenance.metadata.to_string())
        .execute(&mut *tx)
        .await?;
        execution_state_sql::write(self, &mut *tx, run_id, &state, false).await?;
        if let Some(attempt_id) = pipeline_member_attempt_id {
            if pipeline_run_id.is_none() {
                return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                    .error("pipeline member attempt requires an owning pipeline run"));
            }
            let updated = sqlx::query(&self.render(
                "UPDATE pipeline_member_attempts SET workflow_run_id = ?, status = 'running', started_at = COALESCE(started_at, ?) WHERE id = ? AND workflow_run_id IS NULL",
            ))
            .bind(run_id)
            .bind(now)
            .bind(attempt_id)
            .execute(&mut *tx)
            .await?;
            if updated.affected() != 1 {
                return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(format!(
                    "pipeline member attempt {attempt_id} is missing or already bound"
                )));
            }
        }
        sqlx::query(&self.render(
            "INSERT INTO workflow_vm_modules (workflow_run_id, version, module_json, created_at) VALUES (?, ?, ?, ?)",
        ))
        .bind(run_id)
        .bind(i64::from(module.version))
        .bind(serde_json::to_string(&module)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // Timer handlers are frozen into the compiled module. Materialize one run-scoped schedule
        // per declaration before exposing the run, so an engine restart can discover and arm the
        // first occurrence without reading mutable authoring metadata.
        for handler in module
            .interrupt_handlers
            .iter()
            .filter(|handler| handler.source == InterruptSource::Timer)
        {
            let (Some(timer_id), Some(interval_seconds)) =
                (handler.timer_id.as_deref(), handler.interval_seconds)
            else {
                continue;
            };
            if interval_seconds <= 0 {
                continue;
            }
            sqlx::query(&self.render(
                "INSERT INTO workflow_timer_interrupts (workflow_run_id, timer_id, interval_seconds, next_due_at) VALUES (?, ?, ?, ?)",
            ))
            .bind(run_id)
            .bind(timer_id)
            .bind(interval_seconds)
            .bind(now.saturating_add(interval_seconds))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render(
            "INSERT INTO workflow_continuations (id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(continuation.id)
        .bind(run_id)
        .bind(i64::from(continuation.module_version))
        .bind(serde_json::to_string(&continuation)?)
        .bind(wire_name(&continuation.status)?)
        .bind(continuation.revision as i64)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
        .bind(run_id)
        .bind(0_i64)
        .bind(Some(continuation.id))
        .bind(Option::<Uuid>::None)
        .bind(serde_json::to_string(&entry)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
        )))
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        let mut run = mappers::row_to_workflow_run(&row);
        run.execution_state = state;
        tx.commit().await?;
        Ok(run)
    }

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
        row.as_ref()
            .map(mappers::row_to_workflow_module)
            .transpose()
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

    async fn fetch_workflow_continuations(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowContinuation>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE workflow_run_id = ? ORDER BY created_at, id"
        )))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_workflow_continuation)
            .collect()
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

    async fn fetch_workflow_effects(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowEffect>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {EFFECT_COLUMNS} FROM workflow_effects WHERE workflow_run_id = ? ORDER BY created_at, id"
        )))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_workflow_effect).collect()
    }

    async fn fetch_workflow_effect_output(
        &self,
        effect_id: Uuid,
    ) -> Result<Vec<WorkflowEffectOutputEvent>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT event_id, effect_id, workflow_run_id, continuation_id, attempt, output_json, created_at FROM workflow_effect_output_events WHERE effect_id = ? ORDER BY created_at, event_id",
        ))
        .bind(effect_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(WorkflowEffectOutputEvent {
                    event_id: row.try_get("event_id")?,
                    effect_id: row.try_get("effect_id")?,
                    workflow_run_id: row.try_get("workflow_run_id")?,
                    continuation_id: row.try_get("continuation_id")?,
                    attempt: row.try_get::<i64, _>("attempt")? as u32,
                    output: serde_json::from_str(&row.try_get::<String, _>("output_json")?)?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    async fn append_workflow_effect_output(
        &self,
        event: WorkflowEffectOutputEvent,
    ) -> Result<bool, SendableError> {
        let effect = self.fetch_workflow_effect(event.effect_id).await?;
        let Some(effect) = effect else {
            return Ok(false);
        };
        if effect.attempt != event.attempt
            || effect.workflow_run_id != event.workflow_run_id
            || effect.continuation_id != event.continuation_id
            || effect.status.is_terminal()
        {
            return Ok(false);
        }
        let sql = self.dialect().insert_ignore(
            "workflow_effect_output_events",
            "event_id, effect_id, workflow_run_id, continuation_id, attempt, output_json, created_at",
            "?, ?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        );
        let inserted = sqlx::query(&self.render(&sql))
            .bind(event.event_id)
            .bind(event.effect_id)
            .bind(event.workflow_run_id)
            .bind(event.continuation_id)
            .bind(i64::from(event.attempt))
            .bind(serde_json::to_string(&event.output)?)
            .bind(event.created_at)
            .execute(self.pool())
            .await?;
        Ok(inserted.affected() > 0)
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

    async fn settle_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "UPDATE workflow_runs SET status = ?, finished_at = ?, message = ? WHERE id = ?",
        ))
        .bind(status.as_str())
        .bind(now)
        .bind(message)
        .bind(workflow_run_id)
        .execute(&mut *tx)
        .await?;
        // a terminal run must not keep holding a named mutex. it is released in the same
        // transaction that settles the run, because a release that could be lost between the two
        // is a permanent deadlock for every later run of the same key.
        release_run_mutexes(self, &mut tx, workflow_run_id, now).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn fetch_unsettled_vm_pipeline_members(
        &self,
        limit: i64,
    ) -> Result<Vec<Uuid>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT DISTINCT r.id FROM workflow_runs r INNER JOIN workflow_vm_modules m ON m.workflow_run_id = r.id INNER JOIN pipeline_member_attempts a ON a.workflow_run_id = r.id WHERE r.pipeline_run_id IS NOT NULL AND r.status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND a.status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled', 'skipped') ORDER BY r.id LIMIT ?",
        ))
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(|row| row.get::<Uuid, _>("id")).collect())
    }

    async fn create_workflow_vm(
        &self,
        module: WorkflowModule,
        mut continuation: WorkflowContinuation,
    ) -> Result<(), SendableError> {
        if !module.is_supported()
            || !continuation.is_supported()
            || continuation.module_version != module.version
        {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot create a workflow VM from incompatible module state"));
        }
        let now = Utc::now().timestamp();
        let node_entries = std::mem::take(&mut continuation.pending_node_entries);
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
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
        .bind(continuation.workflow_run_id)
        .bind(0_i64)
        .bind(Some(continuation.id))
        .bind(Option::<Uuid>::None)
        .bind(serde_json::to_string(&entry)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        append_node_entries(
            self,
            &mut tx,
            continuation.workflow_run_id,
            continuation.id,
            node_entries,
            now,
        )
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
        let node_entries = std::mem::take(&mut continuation.pending_node_entries);
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
            "INSERT INTO workflow_effects (id, version, workflow_run_id, continuation_id, sequence, attempt, request_json, status, current_executor_replica_id, last_executor_replica_id, result_json, message, idempotency_key, created_at, updated_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(effect.id)
        .bind(i64::from(effect.version))
        .bind(effect.workflow_run_id)
        .bind(effect.continuation_id)
        .bind(effect.sequence as i64)
        .bind(i64::from(effect.attempt))
        .bind(serde_json::to_string(&effect.request)?)
        .bind(wire_name(&effect.status)?)
        .bind(effect.current_executor_replica_id)
        .bind(effect.last_executor_replica_id)
        .bind(effect.result.as_ref().map(serde_json::to_string).transpose()?)
        .bind(effect.message.clone())
        .bind(effect.idempotency_key())
        .bind(effect.created_at)
        .bind(effect.updated_at)
        .bind(effect.finished_at)
        .execute(&mut *tx)
        .await?;
        append_node_entries(
            self,
            &mut tx,
            effect.workflow_run_id,
            continuation.id,
            node_entries,
            effect.created_at,
        )
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
            // `yield_effect` advances the continuation before it is persisted. The preceding
            // opcode is the stable source-map location of the graph node that issued this effect.
            instruction_pointer: Some(continuation.instruction_pointer.saturating_sub(1)),
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
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
        mut children: Vec<WorkflowContinuation>,
        join_key: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let parent_node_entries = std::mem::take(&mut parent.pending_node_entries);
        let child_node_entries: Vec<_> = children
            .iter_mut()
            .map(|child| (child.id, std::mem::take(&mut child.pending_node_entries)))
            .collect();
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
        append_node_entries(
            self,
            &mut tx,
            parent.workflow_run_id,
            parent.id,
            parent_node_entries,
            now,
        )
        .await?;
        for (continuation_id, node_entries) in child_node_entries {
            append_node_entries(
                self,
                &mut tx,
                parent.workflow_run_id,
                continuation_id,
                node_entries,
                now,
            )
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
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
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

    async fn commit_workflow_continuation(
        &self,
        mut continuation: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> Result<(), SendableError> {
        if !continuation.is_supported() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot commit an unsupported workflow continuation"));
        }
        let now = Utc::now().timestamp();
        let node_entries = std::mem::take(&mut continuation.pending_node_entries);
        let expected = continuation.revision;
        continuation.revision += 1;
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = CASE WHEN ? = 'runnable' THEN ? ELSE NULL END, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&continuation)?)
        .bind(wire_name(&continuation.status)?)
        .bind(continuation.revision as i64)
        .bind(wire_name(&continuation.status)?)
        .bind(now)
        .bind(now)
        .bind(continuation.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        append_node_entries(
            self,
            &mut tx,
            continuation.workflow_run_id,
            continuation.id,
            node_entries,
            now,
        )
        .await?;
        let lock = match self.dialect() {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres | SqlDialect::MySql => " FOR UPDATE",
        };
        sqlx::query(&self.render(&format!("SELECT id FROM workflow_runs WHERE id = ?{lock}")))
            .bind(continuation.workflow_run_id)
            .fetch_one(&mut *tx)
            .await?;
        let sequence: i64 = sqlx::query_scalar(&self.render(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM workflow_journal_entries WHERE workflow_run_id = ?",
        ))
        .bind(continuation.workflow_run_id)
        .fetch_one(&mut *tx)
        .await?;
        let effect_id = match &journal {
            WorkflowJournalEntry::EffectRequested { effect_id, .. }
            | WorkflowJournalEntry::EffectSettled { effect_id, .. } => Some(*effect_id),
            _ => None,
        };
        sqlx::query(&self.render(
            "INSERT INTO workflow_journal_entries (id, version, workflow_run_id, sequence, continuation_id, effect_id, entry_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
        .bind(continuation.workflow_run_id)
        .bind(sequence)
        .bind(Some(continuation.id))
        .bind(effect_id)
        .bind(serde_json::to_string(&journal)?)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn raise_workflow_interrupt(
        &self,
        mut suspended: WorkflowContinuation,
        mut handler: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> Result<(), SendableError> {
        if !suspended.is_supported() || !handler.is_supported() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot raise an interrupt from an unsupported continuation"));
        }
        let now = Utc::now().timestamp();
        let suspended_node_entries = std::mem::take(&mut suspended.pending_node_entries);
        let handler_node_entries = std::mem::take(&mut handler.pending_node_entries);
        let expected = suspended.revision;
        suspended.revision += 1;
        let mut tx = self.pool().begin().await?;
        // the freeze is compare-and-swapped like every other transition: a drive that lost a race
        // must reread rather than suspend a thread that has already moved.
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = 'suspended', version = ?, ready_at = NULL, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&suspended)?)
        .bind(suspended.revision as i64)
        .bind(now)
        .bind(suspended.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        // the handler id is derived from the interrupted thread and the source, so a redelivered
        // drive re-raising the same interrupt inserts nothing rather than a second handler.
        sqlx::query(&self.render(&self.dialect().insert_ignore(
            "workflow_continuations",
            "id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at",
            "?, ?, ?, ?, ?, ?, ?, ?, ?",
            "id",
            None,
        )))
        .bind(handler.id)
        .bind(handler.workflow_run_id)
        .bind(i64::from(handler.module_version))
        .bind(serde_json::to_string(&handler)?)
        .bind(wire_name(&handler.status)?)
        .bind(handler.revision as i64)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        append_node_entries(
            self,
            &mut tx,
            suspended.workflow_run_id,
            suspended.id,
            suspended_node_entries,
            now,
        )
        .await?;
        append_node_entries(
            self,
            &mut tx,
            handler.workflow_run_id,
            handler.id,
            handler_node_entries,
            now,
        )
        .await?;
        append_journal(
            self,
            &mut tx,
            suspended.workflow_run_id,
            Some(suspended.id),
            &journal,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn start_workflow_interrupt_handler(
        &self,
        mut handler: WorkflowContinuation,
        journal: WorkflowJournalEntry,
    ) -> Result<(), SendableError> {
        if !handler.is_supported() {
            return Err(crate::errors::WORKFLOW_VM_CORRUPT_STATE
                .error("cannot start a handler from an unsupported continuation"));
        }
        let now = Utc::now().timestamp();
        let node_entries = std::mem::take(&mut handler.pending_node_entries);
        let mut tx = self.pool().begin().await?;
        let inserted = sqlx::query(&self.render(&self.dialect().insert_ignore(
            "workflow_continuations",
            "id, workflow_run_id, module_version, continuation_json, status, version, ready_at, created_at, updated_at",
            "?, ?, ?, ?, ?, ?, ?, ?, ?",
            "id",
            None,
        )))
        .bind(handler.id)
        .bind(handler.workflow_run_id)
        .bind(i64::from(handler.module_version))
        .bind(serde_json::to_string(&handler)?)
        .bind(wire_name(&handler.status)?)
        .bind(handler.revision as i64)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // an already-present handler means a redelivered result; do not journal it a second time.
        if inserted.affected() == 0 {
            tx.commit().await?;
            return Ok(());
        }
        append_node_entries(
            self,
            &mut tx,
            handler.workflow_run_id,
            handler.id,
            node_entries,
            now,
        )
        .await?;
        append_journal(
            self,
            &mut tx,
            handler.workflow_run_id,
            Some(handler.id),
            &journal,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn settle_workflow_interrupt(
        &self,
        mut handler: WorkflowContinuation,
        interrupted_continuation_id: Uuid,
        outcome: WorkflowInterruptOutcome,
        journal: WorkflowJournalEntry,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let node_entries = std::mem::take(&mut handler.pending_node_entries);
        let expected = handler.revision;
        handler.revision += 1;
        let mut tx = self.pool().begin().await?;
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = NULL, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&handler)?)
        .bind(wire_name(&handler.status)?)
        .bind(handler.revision as i64)
        .bind(now)
        .bind(handler.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        append_node_entries(
            self,
            &mut tx,
            handler.workflow_run_id,
            handler.id,
            node_entries,
            now,
        )
        .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE id = ?"
        )))
        .bind(interrupted_continuation_id)
        .fetch_optional(&mut *tx)
        .await?;
        // a thread that was cancelled or settled while its handler ran simply stays that way. the
        // handler still retires, which is what stops it being driven forever.
        if let Some(row) = row {
            let mut interrupted = mappers::row_to_workflow_continuation(&row)?;
            if interrupted.status == WorkflowContinuationStatus::Suspended {
                let expected = interrupted.revision;
                interrupted.revision += 1;
                let (status, ready) = match &outcome {
                    WorkflowInterruptOutcome::Resume {
                        instruction_pointer,
                    } => {
                        interrupted.instruction_pointer = *instruction_pointer;
                        interrupted.awaiting_effect_id = None;
                        (WorkflowContinuationStatus::Runnable, true)
                    }
                    WorkflowInterruptOutcome::Fail { .. } => {
                        (WorkflowContinuationStatus::Failed, false)
                    }
                };
                interrupted.status = status;
                sqlx::query(&self.render(
                    "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = ?, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND version = ?",
                ))
                .bind(serde_json::to_string(&interrupted)?)
                .bind(wire_name(&interrupted.status)?)
                .bind(interrupted.revision as i64)
                .bind(ready.then_some(now))
                .bind(now)
                .bind(interrupted.id)
                .bind(expected as i64)
                .execute(&mut *tx)
                .await?;
            }
        }
        append_journal(
            self,
            &mut tx,
            handler.workflow_run_id,
            Some(handler.id),
            &journal,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn request_workflow_interrupt(
        &self,
        workflow_run_id: Uuid,
        continuation_id: Option<Uuid>,
        pending: WorkflowPendingInterrupt,
    ) -> Result<Option<Uuid>, SendableError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE workflow_run_id = ? AND status IN ('runnable', 'waiting', 'paused') ORDER BY created_at, id"
        )))
        .bind(workflow_run_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut target = None;
        for row in &rows {
            let candidate = mappers::row_to_workflow_continuation(row)?;
            let matches = match continuation_id {
                Some(id) => candidate.id == id,
                // untargeted: the run's oldest live thread, which is the one `active_node_id`
                // mirrors for single-position consumers.
                None => true,
            };
            if matches {
                target = Some(candidate);
                break;
            }
        }
        let Some(mut target) = target else {
            tx.rollback().await?;
            return Ok(None);
        };
        let expected = target.revision;
        target.revision += 1;
        target.pending_interrupt = Some(pending);
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, version = ?, updated_at = ? WHERE id = ? AND version = ?",
        ))
        .bind(serde_json::to_string(&target)?)
        .bind(target.revision as i64)
        .bind(now)
        .bind(target.id)
        .bind(expected as i64)
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
        tx.commit().await?;
        Ok(Some(target.id))
    }

    async fn fetch_workflow_timer_interrupts_before(
        &self,
        before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowTimerInterrupt>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT t.workflow_run_id, t.timer_id, t.interval_seconds, t.next_due_at
             FROM workflow_timer_interrupts t
             INNER JOIN workflow_runs r ON r.id = t.workflow_run_id
             WHERE t.next_due_at <= ?
               AND r.status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')
             ORDER BY t.next_due_at, t.workflow_run_id, t.timer_id
             LIMIT ?",
        ))
        .bind(before.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|row| WorkflowTimerInterrupt {
                workflow_run_id: row.get("workflow_run_id"),
                timer_id: row.get("timer_id"),
                interval_seconds: row.get("interval_seconds"),
                due_at: DateTime::<Utc>::from_timestamp(row.get("next_due_at"), 0)
                    .unwrap_or_else(Utc::now),
            })
            .collect())
    }

    async fn fire_workflow_timer_interrupt(
        &self,
        timer: WorkflowTimerInterrupt,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let due_at = timer.due_at.timestamp();
        let now_at = now.timestamp();

        let mut tx = self.pool().begin().await?;
        // A timer may have been canceled with its run or advanced by a competing engine while its
        // broker wake was in flight. The exact due-at guard makes both cases a harmless no-op.
        let active: Option<(String, i64)> = sqlx::query_as(&self.render(
            "SELECT r.status, t.interval_seconds FROM workflow_timer_interrupts t
             INNER JOIN workflow_runs r ON r.id = t.workflow_run_id
             WHERE t.workflow_run_id = ? AND t.timer_id = ? AND t.next_due_at = ?",
        ))
        .bind(timer.workflow_run_id)
        .bind(&timer.timer_id)
        .bind(due_at)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((status, interval_seconds)) = active else {
            tx.commit().await?;
            return Ok(false);
        };
        if matches!(
            status.as_str(),
            "succeeded" | "failed" | "timed_out" | "canceled"
        ) {
            tx.commit().await?;
            return Ok(false);
        }
        if interval_seconds <= 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        let mut next_due_at = due_at.saturating_add(interval_seconds);
        if next_due_at <= now_at {
            let missed = now_at
                .saturating_sub(next_due_at)
                .saturating_div(interval_seconds)
                .saturating_add(1);
            next_due_at = next_due_at.saturating_add(missed.saturating_mul(interval_seconds));
        }

        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations
             WHERE workflow_run_id = ? AND status IN ('runnable', 'waiting', 'paused')
             ORDER BY created_at, id"
        )))
        .bind(timer.workflow_run_id)
        .fetch_all(&mut *tx)
        .await?;
        let target = rows
            .iter()
            .map(mappers::row_to_workflow_continuation)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|continuation| {
                !continuation.is_interrupt_handler() && continuation.pending_interrupt.is_none()
            });

        if let Some(mut continuation) = target {
            let expected = continuation.revision;
            continuation.revision += 1;
            continuation.pending_interrupt = Some(WorkflowPendingInterrupt {
                id: Uuid::now_v7(),
                source: InterruptSource::Timer,
                payload: runinator_models::json!({
                    "timer_id": timer.timer_id.clone(),
                    "due_at": due_at,
                    "interval_seconds": interval_seconds,
                }),
            });
            let updated = sqlx::query(&self.render(
                "UPDATE workflow_continuations SET continuation_json = ?, version = ?, updated_at = ?
                 WHERE id = ? AND version = ?",
            ))
            .bind(serde_json::to_string(&continuation)?)
            .bind(continuation.revision as i64)
            .bind(now_at)
            .bind(continuation.id)
            .bind(expected as i64)
            .execute(&mut *tx)
            .await?;
            if updated.affected() == 0 {
                tx.rollback().await?;
                return Ok(false);
            }
        }

        let advanced = sqlx::query(&self.render(
            "UPDATE workflow_timer_interrupts SET next_due_at = ?
             WHERE workflow_run_id = ? AND timer_id = ? AND next_due_at = ?",
        ))
        .bind(next_due_at)
        .bind(timer.workflow_run_id)
        .bind(&timer.timer_id)
        .bind(due_at)
        .execute(&mut *tx)
        .await?;
        if advanced.affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn claim_workflow_effect_executor(
        &self,
        effect_id: Uuid,
        attempt: u32,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        // guarded on the attempt so a redelivery to a second worker after the first one's attempt
        // was superseded cannot steal the lease from the live executor.
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_effects SET status = 'running', current_executor_replica_id = ?, updated_at = ? WHERE id = ? AND attempt = ? AND status IN ('requested', 'running')",
        ))
        .bind(replica_id)
        .bind(claimed_at.timestamp())
        .bind(effect_id)
        .bind(i64::from(attempt))
        .execute(self.pool())
        .await?;
        Ok(updated.affected() > 0)
    }

    async fn retry_workflow_effect(
        &self,
        effect_id: Uuid,
        attempt: u32,
        available_at: DateTime<Utc>,
        message: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
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
        let mut effect = mappers::row_to_workflow_effect(&row)?;
        if effect.attempt != attempt || effect.status.is_terminal() {
            tx.commit().await?;
            return Ok(false);
        }
        let stamp = now.timestamp();
        // re-arming clears the previous outcome but keeps the executor attribution, exactly as
        // settling does: a retry-exhaustion report still needs to name who ran the attempt before.
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_effects SET attempt = attempt + 1, status = 'requested', result_json = NULL, message = ?, updated_at = ?, finished_at = NULL, last_executor_replica_id = COALESCE(current_executor_replica_id, last_executor_replica_id), current_executor_replica_id = NULL WHERE id = ? AND attempt = ? AND status IN ('requested', 'running')",
        ))
        .bind(message)
        .bind(stamp)
        .bind(effect_id)
        .bind(i64::from(attempt))
        .execute(&mut *tx)
        .await?;
        if updated.affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }
        effect.attempt = attempt + 1;
        // re-publish the *frozen* command rather than rebuilding one from workflow state, bumping
        // only the fields that identify this attempt. `idempotency_key()` carries the attempt, so
        // the worker's claim cannot replay the attempt that just failed.
        let command_json: String = sqlx::query(&self.render(
            "SELECT command_json FROM workflow_effect_dispatches WHERE effect_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
        ))
        .bind(effect_id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("command_json")?;
        let mut command: EffectCommand = serde_json::from_str(&command_json)?;
        command.command_id = Uuid::now_v7();
        command.attempt = effect.attempt;
        command.idempotency_key = effect.idempotency_key();
        sqlx::query(&self.render(
            "INSERT INTO workflow_effect_dispatches (id, effect_id, dedupe_key, command_json, created_at, updated_at, available_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(command.command_id)
        .bind(effect_id)
        .bind(effect.idempotency_key())
        .bind(serde_json::to_string(&command)?)
        .bind(stamp)
        .bind(stamp)
        .bind(available_at.timestamp())
        .execute(&mut *tx)
        .await?;
        append_journal(
            self,
            &mut tx,
            effect.workflow_run_id,
            Some(effect.continuation_id),
            &WorkflowJournalEntry::EffectRetryScheduled {
                effect_id,
                attempt: effect.attempt,
                available_at: available_at.timestamp(),
            },
            stamp,
        )
        .await?;
        tx.commit().await?;
        Ok(true)
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
        let continuation_row = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE id = ?"
        )))
        .bind(effect.continuation_id)
        .fetch_one(&mut *tx)
        .await?;
        let mut continuation = mappers::row_to_workflow_continuation(&continuation_row)?;
        if continuation.status != runinator_models::workflow_vm::WorkflowContinuationStatus::Waiting
        {
            tx.commit().await?;
            return Ok(false);
        }
        let now = settled_at.timestamp();
        // settling releases the executor lease but keeps the attribution: `last_...` is what a
        // retry-exhaustion report and the replica views read after the fact.
        let updated = sqlx::query(&self.render(
            "UPDATE workflow_effects SET status = ?, result_json = ?, message = ?, updated_at = ?, finished_at = ?, last_executor_replica_id = COALESCE(current_executor_replica_id, last_executor_replica_id), current_executor_replica_id = NULL WHERE id = ? AND attempt = ? AND status IN ('requested', 'running')",
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
        let expected_revision = continuation.revision;
        continuation.status = if continuation.operator_paused {
            runinator_models::workflow_vm::WorkflowContinuationStatus::Paused
        } else {
            runinator_models::workflow_vm::WorkflowContinuationStatus::Runnable
        };
        continuation.revision += 1;
        let resumed = sqlx::query(&self.render(
            "UPDATE workflow_continuations SET continuation_json = ?, status = ?, version = ?, ready_at = ?, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ? AND status = 'waiting' AND version = ?",
        ))
        .bind(serde_json::to_string(&continuation)?)
        .bind(wire_name(&continuation.status)?)
        .bind(continuation.revision as i64)
        .bind(now)
        .bind(now)
        .bind(effect.continuation_id)
        .bind(expected_revision as i64)
        .execute(&mut *tx)
        .await?;
        if resumed.affected() == 0 {
            tx.rollback().await?;
            return Err(cas_error());
        }
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
        .bind(i64::from(WORKFLOW_JOURNAL_VERSION))
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

    async fn pause_workflow_vm_run(&self, workflow_run_id: Uuid) -> Result<u64, SendableError> {
        let continuations = self.fetch_workflow_continuations(workflow_run_id).await?;
        let mut changed = 0;
        for mut continuation in continuations {
            if continuation.status.is_terminal()
                || continuation.status == WorkflowContinuationStatus::Joined
            {
                continue;
            }
            continuation.operator_paused = true;
            if continuation.status == WorkflowContinuationStatus::Runnable {
                continuation.status = WorkflowContinuationStatus::Paused;
            }
            if let Some(debug) =
                continuation
                    .frames
                    .iter_mut()
                    .rev()
                    .find_map(|frame| match frame {
                        WorkflowFrame::Debug(debug) => Some(debug),
                        _ => None,
                    })
            {
                debug.paused = true;
                debug.step_requested = false;
            }
            self.commit_workflow_continuation(
                continuation.clone(),
                WorkflowJournalEntry::Transitioned {
                    continuation_id: continuation.id,
                    instruction_pointer: continuation.instruction_pointer,
                },
            )
            .await?;
            changed += 1;
        }
        if changed > 0 {
            sqlx::query(&self.render(
                "UPDATE workflow_runs SET status = 'paused', message = 'Workflow VM paused' WHERE id = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')",
            ))
            .bind(workflow_run_id)
            .execute(self.pool())
            .await?;
        }
        Ok(changed)
    }

    async fn resume_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        step: bool,
    ) -> Result<u64, SendableError> {
        let continuations = self.fetch_workflow_continuations(workflow_run_id).await?;
        let mut changed = 0;
        for mut continuation in continuations {
            if !continuation.operator_paused {
                continue;
            }
            continuation.operator_paused = false;
            if continuation.status == WorkflowContinuationStatus::Paused {
                continuation.status = WorkflowContinuationStatus::Runnable;
            }
            if let Some(debug) =
                continuation
                    .frames
                    .iter_mut()
                    .rev()
                    .find_map(|frame| match frame {
                        WorkflowFrame::Debug(debug) => Some(debug),
                        _ => None,
                    })
            {
                debug.paused = false;
                debug.step_requested = step;
            }
            self.commit_workflow_continuation(
                continuation.clone(),
                WorkflowJournalEntry::Transitioned {
                    continuation_id: continuation.id,
                    instruction_pointer: continuation.instruction_pointer,
                },
            )
            .await?;
            changed += 1;
        }
        if changed > 0 {
            sqlx::query(&self.render(
                "UPDATE workflow_runs SET status = 'running', message = ? WHERE id = ? AND status = 'paused'",
            ))
            .bind(if step { "Workflow VM step requested" } else { "Workflow VM resumed" })
            .bind(workflow_run_id)
            .execute(self.pool())
            .await?;
        }
        Ok(changed)
    }

    async fn cancel_workflow_vm_run(
        &self,
        workflow_run_id: Uuid,
        message: String,
    ) -> Result<Vec<Uuid>, SendableError> {
        let effects = self.fetch_workflow_effects(workflow_run_id).await?;
        let pending_effects = effects
            .iter()
            .filter(|effect| !effect.status.is_terminal())
            .map(|effect| effect.id)
            .collect::<Vec<_>>();
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "UPDATE workflow_effects SET status = 'canceled', message = ?, updated_at = ?, finished_at = ? WHERE workflow_run_id = ? AND status IN ('requested', 'running')",
        ))
        .bind(message.as_str())
        .bind(now)
        .bind(now)
        .bind(workflow_run_id)
        .execute(&mut *tx)
        .await?;
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {CONTINUATION_COLUMNS} FROM workflow_continuations WHERE workflow_run_id = ? AND status NOT IN ('succeeded', 'failed', 'canceled')"
        )))
        .bind(workflow_run_id)
        .fetch_all(&mut *tx)
        .await?;
        for row in rows {
            let mut continuation = mappers::row_to_workflow_continuation(&row)?;
            continuation.status = WorkflowContinuationStatus::Canceled;
            continuation.revision += 1;
            sqlx::query(&self.render(
                "UPDATE workflow_continuations SET continuation_json = ?, status = 'canceled', version = ?, ready_at = NULL, claimed_by = NULL, claimed_until = NULL, updated_at = ? WHERE id = ?",
            ))
            .bind(serde_json::to_string(&continuation)?)
            .bind(continuation.revision as i64)
            .bind(now)
            .bind(continuation.id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render(
            "UPDATE workflow_runs SET status = 'canceled', finished_at = ?, message = ? WHERE id = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')",
        ))
        .bind(now)
        .bind(message)
        .bind(workflow_run_id)
        .execute(&mut *tx)
        .await?;
        release_run_mutexes(self, &mut tx, workflow_run_id, now).await?;
        tx.commit().await?;
        Ok(pending_effects)
    }

    async fn claim_workflow_vm_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        continuation_id: Uuid,
        now: i64,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(&self.dialect().insert_ignore(
            "workflow_mutexes",
            "name, updated_at",
            "?, ?",
            "name",
            None,
        )))
        .bind(name.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // Lock the named row on every dialect before observing its holder.
        sqlx::query(
            &self.render("UPDATE workflow_mutexes SET updated_at = updated_at WHERE name = ?"),
        )
        .bind(name.as_str())
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            &self.render("SELECT holder_run_id, acquired_at FROM workflow_mutexes WHERE name = ?"),
        )
        .bind(name.as_str())
        .fetch_one(&mut *tx)
        .await?;
        let holder = row.try_get::<Option<Uuid>, _>("holder_run_id")?;
        let held_since = row.try_get::<Option<i64>, _>("acquired_at")?;
        // a holder whose run has already finished is a lock nobody will ever release — a crash
        // between the effect settling and the run settling leaves exactly that row. treating it
        // as free is what keeps one lost release from deadlocking the key forever.
        let stale_holder = match holder {
            Some(holder) if holder != workflow_run_id => sqlx::query(&self.render(
                "SELECT 1 AS present FROM workflow_runs WHERE id = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled')",
            ))
            .bind(holder)
            .fetch_optional(&mut *tx)
            .await?
            .is_none(),
            _ => false,
        };
        let acquired = holder.is_none() || holder == Some(workflow_run_id) || stale_holder;
        if acquired {
            // a re-entrant claim by the current holder keeps its original acquisition time; taking
            // the key over from nobody, or from a stale holder, starts the clock now.
            let acquired_at = match holder {
                Some(holder) if holder == workflow_run_id => held_since.unwrap_or(now),
                _ => now,
            };
            sqlx::query(&self.render(
                "UPDATE workflow_mutexes SET holder_run_id = ?, holder_continuation_id = ?, acquired_at = ?, updated_at = ? WHERE name = ?",
            ))
            .bind(workflow_run_id)
            .bind(continuation_id)
            .bind(acquired_at)
            .bind(now)
            .bind(name.as_str())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(acquired)
    }

    async fn release_workflow_vm_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        continuation_id: Uuid,
        now: i64,
    ) -> Result<(), SendableError> {
        // Releasing is intentionally idempotent. The host can be redelivered after it made this
        // database write but before it published the effect result; turning that retry into a
        // failure would leave the continuation parked even though the key is already free.
        sqlx::query(&self.render(
            "UPDATE workflow_mutexes SET holder_run_id = NULL, holder_continuation_id = NULL, acquired_at = NULL, hold_deadline = NULL, overdue_at = NULL, updated_at = ? WHERE name = ? AND holder_run_id = ? AND holder_continuation_id = ?",
        ))
        .bind(now)
        .bind(name)
        .bind(workflow_run_id)
        .bind(continuation_id)
        .execute(self.pool())
        .await?;
        Ok(())
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

    async fn claim_pending_workflow_effect_dispatches(
        &self,
        publisher_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<EffectDispatchRecord>, SendableError> {
        const COLUMNS: &str = "id, effect_id, dedupe_key, command_json, attempts, published_at, created_at, updated_at, last_error, claimed_by, claimed_until";
        let mut tx = self.pool().begin().await?;
        let ids = sqlx::query(&self.render(&format!(
            "SELECT id FROM workflow_effect_dispatches WHERE published_at IS NULL AND available_at <= ? AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?) ORDER BY created_at, id LIMIT ?{}",
            self.dialect().skip_locked()
        )))
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(publisher_id.as_str())
        .bind(limit.max(1))
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<Result<Vec<_>, _>>()?;
        let mut claimed = Vec::with_capacity(ids.len());
        for id in ids {
            let changed = sqlx::query(&self.render(
                "UPDATE workflow_effect_dispatches SET claimed_by = ?, claimed_until = ?, updated_at = ? WHERE id = ? AND published_at IS NULL AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)",
            ))
            .bind(publisher_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(id)
            .bind(now.timestamp())
            .bind(publisher_id.as_str())
            .execute(&mut *tx)
            .await?;
            if changed.affected() == 0 {
                continue;
            }
            let row = sqlx::query(&self.render(&format!(
                "SELECT {COLUMNS} FROM workflow_effect_dispatches WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            claimed.push(mappers::row_to_workflow_effect_dispatch(&row)?);
        }
        tx.commit().await?;
        Ok(claimed)
    }

    async fn mark_workflow_effect_dispatch_published(
        &self,
        dispatch_id: Uuid,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE workflow_effect_dispatches SET published_at = ?, updated_at = ?, last_error = NULL, claimed_by = NULL, claimed_until = NULL WHERE id = ? AND published_at IS NULL",
        ))
        .bind(now)
        .bind(now)
        .bind(dispatch_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn mark_workflow_effect_dispatch_failed(
        &self,
        dispatch_id: Uuid,
        error: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE workflow_effect_dispatches SET attempts = attempts + 1, updated_at = ?, last_error = ?, claimed_by = NULL, claimed_until = NULL WHERE id = ? AND published_at IS NULL",
        ))
        .bind(now)
        .bind(error)
        .bind(dispatch_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
