//! every `DatabaseImpl` method, written once over any `SqlBackend`.
//!
//! the bodies are authored in sqlite-style `?` placeholders and rendered per dialect; the handful of
//! genuinely divergent fragments (boolean literal, row locking, insert-or-ignore form, and the
//! postgres no-id insert path) are the only places that branch on `self.dialect()`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use runinator_comm::{
    ActionCommand, ActionDispatchRecord, WorkflowResultEvent, WorkflowResultEventKind,
};
use runinator_models::cursor::RunCursor;
use runinator_models::value::Value;
use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, AuthContext, AuthSession, Grant, LocalCredential, Team, User},
    billing::{OrgQuota, OrgResourceGroup, UsageSample},
    errors::SendableError,
    notifications::{
        NewNotification, NewNotificationPolicy, Notification, NotificationChannel,
        NotificationDelivery, NotificationDeliveryStatus, NotificationEvent, NotificationPolicy,
    },
    orchestration::{
        IdempotencyClaim, NewOrchestrationEvent, NodeTransition, NodeTransitionStat,
        OrchestrationEvent, ReadyNodeRecord,
    },
    orgs::{OrgMembership, OrgRole, Organization},
    pipelines::{Pipeline, PipelineRun, PipelineTrigger},
    replicas::{
        ReplicaHeartbeatRequest, ReplicaKind, ReplicaProviderRegistration,
        ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
        ReplicaStatus, WorkflowRunProvenance,
    },
    revisions::WorkflowRevision,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    schedules::{
        BackfillRequest, BackfillResponse, CatchupPolicy, ConcurrencyPolicy,
        DEFAULT_BACKFILL_LIMIT, FiringOutcome, FreezeWindow, MAX_BACKFILL_LIMIT, NewFreezeWindow,
        TriggerCatchup, TriggerFiringBatch, WorkflowConcurrency,
    },
    settings::{SettingKind, SettingRecord},
    telemetry::ReplicaSample,
    workflows::{
        NewWorkflowRunArtifact, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
    },
};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Row, Type};
use uuid::Uuid;

use crate::{
    archive::{ArchiveMark, ArchiveRow, ArchiveTable},
    backend::{RowsAffected, SqlBackend, SqlStore},
    common::{
        PipelineTriggerExt, WorkflowTriggerExt, cron_slots_between, json_metadata, json_opt_i64,
        json_opt_str, json_opt_uuid, json_str, next_execution_for_cron, status_list,
        workflow_result_event_type,
    },
    mappers,
    queries::SqlDialect,
};
use runinator_store::prelude::*;

const WORKFLOW_RUN_COLUMNS: &str = "id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, state_version, created_at, started_at, finished_at, message, name, correlation_key, pipeline_run_id, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata";
const WORKFLOW_NODE_RUN_COLUMNS: &str = "id, workflow_run_id, node_id, cursor_id, speculative, status, attempt, parameters, output_json, state, transition_reason, prev_node_run_id, created_at, started_at, finished_at, message, current_executor_replica_id, last_executor_replica_id, executor_claimed_at, executor_released_at";
/// every column `mappers::row_to_ready_node` reads. hoisted because this list appeared verbatim in
/// seven places, and a mapper reading a column one of them forgot to select panics only on that one
/// code path.
pub(super) const READY_NODE_COLUMNS: &str = "id, source_event_id, workflow_run_id, node_id, cursor_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at";
const REPLICA_COLUMNS: &str = "replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at, registered_by_principal_id, registered_by_kind, registered_by_org_id";
const REPLICA_PROVIDER_COLUMNS: &str = "replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at";
const PIPELINE_COLUMNS: &str =
    "id, name, description, org_id, workflow_ids, defaults, metadata, created_at, updated_at";
const PIPELINE_TRIGGER_COLUMNS: &str = "id, pipeline_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at";
const PIPELINE_RUN_COLUMNS: &str = "id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, started_at, finished_at, message, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata";

const NOTIFICATION_POLICY_COLUMNS: &str = "id, workflow_id, name, event, severity, channel, target, threshold_seconds, enabled, managed_by, configuration, created_at, updated_at";
const NOTIFICATION_DELIVERY_COLUMNS: &str = "id, notification_id, policy_id, channel, target, status, attempts, last_error, created_at, updated_at";

/// true when an insert lost a unique-constraint race rather than failing for a reason worth
/// surfacing. lets a caller that assigns its own sequence number recompute and retry.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// shared insert for the create and pack-reconcile paths, which differ only in how the id is chosen.
trait NotificationSqlExt: SqlBackend {
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
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
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
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(id)
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
        .execute(self.pool())
        .await?;
        Ok(())
    }
}

const FREEZE_WINDOW_COLUMNS: &str =
    "id, org_id, workflow_id, name, reason, starts_at, ends_at, enabled, created_at, updated_at";

/// which freeze windows apply to a `workflow_triggers` row: the ones naming its workflow, plus the
/// blanket windows for its org and for the platform.
const WORKFLOW_FREEZE_SCOPE: &str = "(f.workflow_id IS NULL OR f.workflow_id = workflow_triggers.workflow_id) AND (f.org_id IS NULL OR f.org_id = (SELECT w.org_id FROM workflows w WHERE w.id = workflow_triggers.workflow_id))";

/// which freeze windows apply to a `pipeline_triggers` row. a window naming one workflow does not
/// freeze a pipeline: it is the member workflow's own schedule that is frozen, not the pipeline's.
const PIPELINE_FREEZE_SCOPE: &str = "f.workflow_id IS NULL AND (f.org_id IS NULL OR f.org_id = (SELECT p.org_id FROM pipelines p WHERE p.id = pipeline_triggers.pipeline_id))";

/// a correlated `EXISTS` body over the freeze windows in effect at a bound timestamp. binds two
/// copies of `now` (window start, window end); `scope` decides which windows reach the outer row.
fn active_freeze_window_sql(dialect: SqlDialect, scope: &str) -> String {
    format!(
        "SELECT 1 FROM freeze_windows f WHERE f.enabled = {} AND f.starts_at <= ? AND f.ends_at > ? AND {scope}",
        dialect.bool_true(),
    )
}

/// the per-slot steps of a cron firing, shared by the trigger loop and the manual backfill so both
/// paths record firings, snapshot workflows, and start runs the same way.
trait ScheduleSqlExt: SqlBackend {
    /// how many of a workflow's runs have not reached a terminal state.
    async fn active_run_count(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
    ) -> Result<i64, SendableError>;

    /// set every non-terminal run of a workflow to `canceled`, returning the ids. the caller still
    /// has to tell the workers holding those runs' actions; this only settles durable state.
    async fn cancel_active_runs(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, SendableError>;

    /// claim a slot by recording its firing. `false` means another replica already claimed it.
    async fn claim_firing_slot(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger_id: Uuid,
        fire_key: &str,
        scheduler_id: &str,
        outcome: FiringOutcome,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError>;

    /// create the run for a claimed slot and point the firing row at it.
    async fn insert_trigger_run(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger: &WorkflowTrigger,
        snapshot: &WorkflowDefinition,
        scheduler_id: &str,
        slot: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<WorkflowRun, SendableError>;
}

impl<B> ScheduleSqlExt for B
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
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
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn active_run_count(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
    ) -> Result<i64, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT COUNT(*) AS active FROM workflow_runs WHERE workflow_id = ? AND status NOT IN ({})",
            status_list(&WorkflowStatus::TERMINAL),
        )))
        .bind(workflow_id)
        .fetch_one(conn)
        .await?;
        Ok(row.get::<i64, _>("active"))
    }

    async fn cancel_active_runs(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        workflow_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<Uuid>, SendableError> {
        let terminal = status_list(&WorkflowStatus::TERMINAL);
        let rows = sqlx::query(&self.render(&format!(
            "SELECT id FROM workflow_runs WHERE workflow_id = ? AND status NOT IN ({terminal})"
        )))
        .bind(workflow_id)
        .fetch_all(&mut *conn)
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.get::<Uuid, _>("id")).collect();
        if ids.is_empty() {
            return Ok(ids);
        }

        sqlx::query(&self.render(&format!(
            "UPDATE workflow_runs SET status = ?, finished_at = ?, message = ? WHERE workflow_id = ? AND status NOT IN ({terminal})"
        )))
        .bind(WorkflowStatus::Canceled.as_str())
        .bind(now.timestamp())
        .bind("Canceled by a newer run of this workflow (cancel_previous concurrency policy)")
        .bind(workflow_id)
        .execute(conn)
        .await?;

        Ok(ids)
    }

    async fn claim_firing_slot(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger_id: Uuid,
        fire_key: &str,
        scheduler_id: &str,
        outcome: FiringOutcome,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let sql = self.render(&self.dialect().insert_ignore(
            "workflow_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, outcome, created_at",
            "?, ?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let insert = sqlx::query(&sql)
            .bind(Uuid::now_v7())
            .bind(trigger_id)
            .bind(fire_key)
            .bind(scheduler_id)
            .bind(outcome.as_str())
            .bind(now.timestamp())
            .execute(conn)
            .await?;
        Ok(insert.affected() > 0)
    }

    async fn insert_trigger_run(
        &self,
        conn: &mut <Self::Db as Database>::Connection,
        trigger: &WorkflowTrigger,
        snapshot: &WorkflowDefinition,
        scheduler_id: &str,
        slot: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<WorkflowRun, SendableError> {
        let Some(trigger_id) = trigger.id else {
            return Err(crate::errors::TRIGGER_MISSING_ID.bare());
        };
        let new_run_id = Uuid::now_v7();
        let snapshot_json = serde_json::to_string(snapshot)?;
        let parameters = trigger.trigger_parameters().to_string();
        let state = trigger.trigger_state_for_slot(slot).to_string();
        let insert_sql = "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, ?)";
        let run_row = if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(insert_sql))
                .bind(new_run_id)
                .bind(trigger.workflow_id)
                .bind(&snapshot_json)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(&parameters)
                .bind(&state)
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id)
                .bind(trigger.metadata.to_string())
                .execute(&mut *conn)
                .await?;
            sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
            )))
            .bind(new_run_id)
            .fetch_one(&mut *conn)
            .await?
        } else {
            sqlx::query(&self.render(&format!("{insert_sql} RETURNING {WORKFLOW_RUN_COLUMNS}")))
                .bind(new_run_id)
                .bind(trigger.workflow_id)
                .bind(&snapshot_json)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(&parameters)
                .bind(&state)
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id)
                .bind(trigger.metadata.to_string())
                .fetch_one(&mut *conn)
                .await?
        };
        let run = mappers::row_to_workflow_run(&run_row);

        sqlx::query(&self.render(
            "UPDATE workflow_trigger_firings SET workflow_run_id = ? WHERE trigger_id = ? AND fire_key = ?",
        ))
        .bind(run.id)
        .bind(trigger_id)
        .bind(slot.timestamp().to_string())
        .execute(conn)
        .await?;

        Ok(run)
    }
}

trait ArchiveSqlExt: SqlBackend {
    async fn archive_candidate_ids(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<(Uuid, DateTime<Utc>)>, SendableError>;

    async fn fetch_archive_row(
        &self,
        mark: &ArchiveMark,
    ) -> Result<Option<ArchiveRow>, SendableError>;
}

impl<B> ArchiveSqlExt for B
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    async fn archive_candidate_ids(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<(Uuid, DateTime<Utc>)>, SendableError> {
        let sql = table.archive_candidate_sql();
        let rows = sqlx::query(&self.render(sql))
            .bind(eligible_before.timestamp())
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|row| {
                let id: Uuid = row.get("id");
                let created_at: i64 = row.get("created_at");
                Ok((id, timestamp_to_utc(created_at)?))
            })
            .collect()
    }

    async fn fetch_archive_row(
        &self,
        mark: &ArchiveMark,
    ) -> Result<Option<ArchiveRow>, SendableError> {
        let Some(row) = sqlx::query(&self.render(&mark.table.archive_source_sql(self.dialect())))
            .bind(mark.primary_key)
            .fetch_optional(self.pool())
            .await?
        else {
            return Ok(None);
        };
        let row_json = mark.table.archive_row_json(&row)?;
        Ok(Some(ArchiveRow {
            mark_id: mark.id,
            table: mark.table,
            primary_key: mark.primary_key,
            created_at: mark.created_at,
            row: row_json,
        }))
    }
}

/// sql/row-mapping for one archive table. a local trait since `ArchiveTable` lives in
/// `runinator-store`, which stays free of sql text.
trait ArchiveTableSql {
    fn archive_candidate_sql(self) -> &'static str;
    fn archive_source_sql(self, dialect: SqlDialect) -> String;
    fn archive_row_json<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>;
}

impl ArchiveTableSql for ArchiveTable {
    fn archive_candidate_sql(self) -> &'static str {
        match self {
        ArchiveTable::WorkflowRuns => {
            "SELECT id, created_at FROM workflow_runs
             WHERE created_at <= ?
               AND status IN ('succeeded', 'failed', 'timed_out', 'canceled')
               AND NOT EXISTS (SELECT 1 FROM workflow_node_runs WHERE workflow_node_runs.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_ready_nodes WHERE workflow_ready_nodes.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_orchestration_events WHERE workflow_orchestration_events.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_result_events WHERE workflow_result_events.workflow_run_id = workflow_runs.id)
               AND NOT EXISTS (SELECT 1 FROM workflow_trigger_firings WHERE workflow_trigger_firings.workflow_run_id = workflow_runs.id)
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::WorkflowNodeChunks => {
            "SELECT id, created_at FROM workflow_node_chunks
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::WorkflowReadyNodes => {
            "SELECT id, created_at FROM workflow_ready_nodes
             WHERE completed_at IS NOT NULL AND created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::RunChunks => {
            "SELECT id, created_at FROM run_chunks
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::WorkflowActionDispatches => {
            "SELECT id, created_at FROM workflow_action_dispatches
             WHERE published_at IS NOT NULL AND updated_at <= ?
             ORDER BY updated_at, id
             LIMIT ?"
        }
        ArchiveTable::Notifications => {
            "SELECT id, created_at FROM notifications
             WHERE read_at IS NOT NULL AND created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::DeadLetters => {
            "SELECT id, created_at FROM dead_letters
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::AuditLog => {
            "SELECT id, created_at FROM audit_log
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
        ArchiveTable::IdempotencyKeys => {
            "SELECT id, created_at FROM idempotency_keys
             WHERE created_at <= ?
             ORDER BY created_at, id
             LIMIT ?"
        }
    }
    }

    fn archive_source_sql(self, dialect: SqlDialect) -> String {
        match self {
        ArchiveTable::WorkflowRuns => {
            "SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata FROM workflow_runs WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowNodeChunks => {
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowReadyNodes => {
            format!(
                "SELECT {READY_NODE_COLUMNS} FROM workflow_ready_nodes WHERE id = ? AND completed_at IS NOT NULL"
            )
        }
        ArchiveTable::RunChunks => {
            "SELECT id, run_id, sequence, stream, content, created_at FROM run_chunks WHERE id = ?"
                .to_string()
        }
        ArchiveTable::WorkflowActionDispatches => {
            "SELECT id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until FROM workflow_action_dispatches WHERE id = ? AND published_at IS NOT NULL".to_string()
        }
        ArchiveTable::Notifications => {
            "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE id = ? AND read_at IS NOT NULL".to_string()
        }
        ArchiveTable::DeadLetters => {
            "SELECT id, channel, event_id, dedupe_key, attempts, error, payload, created_at FROM dead_letters WHERE id = ?".to_string()
        }
        ArchiveTable::AuditLog => {
            "SELECT id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at FROM audit_log WHERE id = ?".to_string()
        }
        ArchiveTable::IdempotencyKeys => {
            format!(
                "SELECT id, scope, {key_col}, result, created_at FROM idempotency_keys WHERE id = ?",
                key_col = dialect.ident("key")
            )
        }
    }
    }

    fn archive_row_json<R>(self, row: &R) -> Result<Value, SendableError>
    where
        R: Row,
        for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<i64>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<String>: Decode<'r, R::Database> + Type<R::Database>,
        for<'r> Option<Uuid>: Decode<'r, R::Database> + Type<R::Database>,
        for<'c> &'c str: ColumnIndex<R>,
    {
        Ok(match self {
            ArchiveTable::WorkflowRuns => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_id": row.get::<Uuid, _>("workflow_id").to_string(),
                "workflow_snapshot": row.get::<Option<String>, _>("workflow_snapshot"),
                "status": row.get::<String, _>("status"),
                "active_node_id": row.get::<Option<String>, _>("active_node_id"),
                "parameters": row.get::<String, _>("parameters"),
                "state": row.get::<String, _>("state"),
                "created_at": row.get::<i64, _>("created_at"),
                "started_at": row.get::<Option<i64>, _>("started_at"),
                "finished_at": row.get::<Option<i64>, _>("finished_at"),
                "message": row.get::<Option<String>, _>("message"),
                "name": row.get::<Option<String>, _>("name"),
                "trigger_source_kind": row.get::<Option<String>, _>("trigger_source_kind"),
                "trigger_actor_type": row.get::<Option<String>, _>("trigger_actor_type"),
                "trigger_actor_replica_id": row.get::<Option<Uuid>, _>("trigger_actor_replica_id").map(|id| id.to_string()),
                "trigger_actor_display_name": row.get::<Option<String>, _>("trigger_actor_display_name"),
                "trigger_request_host": row.get::<Option<String>, _>("trigger_request_host"),
                "trigger_request_ip": row.get::<Option<String>, _>("trigger_request_ip"),
                "trigger_metadata": row.get::<String, _>("trigger_metadata"),
            }),
            ArchiveTable::WorkflowNodeChunks => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_node_run_id": row.get::<Uuid, _>("workflow_node_run_id").to_string(),
                "sequence": row.get::<i64, _>("sequence"),
                "stream": row.get::<String, _>("stream"),
                "content": row.get::<String, _>("content"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::WorkflowReadyNodes => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "source_event_id": row.get::<Uuid, _>("source_event_id").to_string(),
                "workflow_run_id": row.get::<Uuid, _>("workflow_run_id").to_string(),
                "node_id": row.get::<String, _>("node_id"),
                "status": row.get::<String, _>("status"),
                "ready_at": row.get::<i64, _>("ready_at"),
                "attempts": row.get::<i64, _>("attempts"),
                "claimed_by": row.get::<Option<String>, _>("claimed_by"),
                "claimed_until": row.get::<Option<i64>, _>("claimed_until"),
                "completed_at": row.get::<Option<i64>, _>("completed_at"),
                "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
            }),
            ArchiveTable::RunChunks => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "run_id": row.get::<Uuid, _>("run_id").to_string(),
                "sequence": row.get::<i64, _>("sequence"),
                "stream": row.get::<String, _>("stream"),
                "content": row.get::<String, _>("content"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::WorkflowActionDispatches => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "dedupe_key": row.get::<String, _>("dedupe_key"),
                "command_json": row.get::<String, _>("command_json"),
                "attempts": row.get::<i64, _>("attempts"),
                "created_at": row.get::<i64, _>("created_at"),
                "updated_at": row.get::<i64, _>("updated_at"),
                "published_at": row.get::<Option<i64>, _>("published_at"),
                "last_error": row.get::<Option<String>, _>("last_error"),
                "claimed_by": row.get::<Option<String>, _>("claimed_by"),
                "claimed_until": row.get::<Option<i64>, _>("claimed_until"),
            }),
            ArchiveTable::Notifications => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "workflow_run_id": row.get::<Option<Uuid>, _>("workflow_run_id").map(|id| id.to_string()),
                "workflow_node_id": row.get::<Option<String>, _>("workflow_node_id"),
                "channel": row.get::<String, _>("channel"),
                "severity": row.get::<String, _>("severity"),
                "title": row.get::<String, _>("title"),
                "body": row.get::<Option<String>, _>("body"),
                "target": row.get::<Option<String>, _>("target"),
                "metadata": row.get::<String, _>("metadata"),
                "read_at": row.get::<Option<i64>, _>("read_at"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::DeadLetters => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "channel": row.get::<String, _>("channel"),
                "event_id": row.get::<Option<Uuid>, _>("event_id").map(|id| id.to_string()),
                "dedupe_key": row.get::<Option<String>, _>("dedupe_key"),
                "attempts": row.get::<i64, _>("attempts"),
                "error": row.get::<String, _>("error"),
                "payload": row.get::<String, _>("payload"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::AuditLog => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "actor_id": row.get::<Option<Uuid>, _>("actor_id").map(|id| id.to_string()),
                "actor_kind": row.get::<String, _>("actor_kind"),
                "action": row.get::<String, _>("action"),
                "resource_type": row.get::<Option<String>, _>("resource_type"),
                "resource_id": row.get::<Option<Uuid>, _>("resource_id").map(|id| id.to_string()),
                "outcome": row.get::<String, _>("outcome"),
                "detail": row.get::<Option<String>, _>("detail"),
                "metadata": row.get::<String, _>("metadata"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
            ArchiveTable::IdempotencyKeys => runinator_models::json!({
                "id": row.get::<Uuid, _>("id").to_string(),
                "scope": row.get::<String, _>("scope"),
                "key": row.get::<String, _>("key"),
                "result": row.get::<String, _>("result"),
                "created_at": row.get::<i64, _>("created_at"),
            }),
        })
    }
}

fn timestamp_to_utc(timestamp: i64) -> Result<DateTime<Utc>, SendableError> {
    DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid unix timestamp {timestamp}"),
        )) as SendableError
    })
}

/// read a claim upsert's row back as the outcome the caller acts on. a completed row is a replayable
/// result whoever owns it; otherwise the owner decides between acquiring and losing.
fn row_to_idempotency_claim<R>(row: &R, owner_node_run_id: Uuid) -> IdempotencyClaim
where
    R: Row,
    for<'a> &'a str: ColumnIndex<R>,
    for<'a> Option<Uuid>: Decode<'a, R::Database> + Type<R::Database>,
    for<'a> Option<i64>: Decode<'a, R::Database> + Type<R::Database>,
    for<'a> String: Decode<'a, R::Database> + Type<R::Database>,
{
    let completed_at: Option<i64> = row.get("completed_at");
    if completed_at.is_some() {
        let raw: String = row.get("result");
        let result = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
        return IdempotencyClaim::Completed { result };
    }
    match row.get::<Option<Uuid>, _>("owner_node_run_id") {
        Some(owner) if owner != owner_node_run_id => IdempotencyClaim::Held {
            owner_node_run_id: owner,
        },
        _ => IdempotencyClaim::Acquired,
    }
}

fn row_to_archive_mark<R>(row: &R) -> Result<ArchiveMark, SendableError>
where
    R: Row,
    for<'r> Uuid: Decode<'r, R::Database> + Type<R::Database>,
    for<'r> String: Decode<'r, R::Database> + Type<R::Database>,
    for<'r> i64: Decode<'r, R::Database> + Type<R::Database>,
    for<'c> &'c str: ColumnIndex<R>,
{
    let table_name: String = row.get("table_name");
    let primary_key: String = row.get("primary_key");
    let table = table_name
        .parse::<ArchiveTable>()
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err)) })?;
    let primary_key = Uuid::parse_str(&primary_key)
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err)) })?;
    Ok(ArchiveMark {
        id: row.get("id"),
        table,
        primary_key,
        created_at: timestamp_to_utc(row.get("created_at"))?,
        archive_day: row.get("archive_day"),
    })
}

// `DatabaseImpl` is foreign (it lives in the sqlx-free `runinator-store`), so the orphan rule
// forbids implementing it on a bare `B`. `SqlStore<B>` is local and forwards `SqlBackend`, which
// keeps every body below generic over the driver exactly as before.
// the subset the workflow state machine calls. split out so `runinator-reducer` can bound on
// `ReducerStore` instead of the whole store; the bodies are unchanged and still generic over the
// driver. the where clause below is repeated verbatim from the `DatabaseImpl` impl: both need the
// same sqlx encode/decode bounds, and spelling them out beats hiding them in a macro.

// the generic implementation, one file per role trait.
mod archive;
mod auth;
mod automation;
mod database_impl;
mod definitions;
mod dispatch;
mod notifications;
mod orgs;
mod reducer;
mod replicas;
mod runs;
mod schedules;
mod settings;
mod task_runs;
