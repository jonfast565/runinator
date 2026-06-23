//! every `DatabaseImpl` method, written once over any `SqlBackend`.
//!
//! the bodies are authored in sqlite-style `?` placeholders and rendered per dialect; the handful of
//! genuinely divergent fragments (boolean literal, row locking, insert-or-ignore form, and the
//! postgres no-id insert path) are the only places that branch on `self.dialect()`.

use chrono::{DateTime, Utc};
use runinator_comm::{
    ActionCommand, ActionDispatchRecord, WorkflowResultEvent, WorkflowResultEventKind,
};
use runinator_models::value::Value;
use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, AuthSession, Grant, LocalCredential, Team, User},
    errors::SendableError,
    notifications::{NewNotification, Notification},
    orchestration::{NewOrchestrationEvent, OrchestrationEvent, ReadyNodeRecord},
    replicas::{
        ReplicaHeartbeatRequest, ReplicaKind, ReplicaProviderRegistration,
        ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
        ReplicaStatus, WorkflowRunProvenance,
    },
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    settings::{SettingKind, SettingRecord},
    workflows::{
        NewWorkflowRunArtifact, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
    },
};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Row, Type};
use uuid::Uuid;

use crate::{
    archive::{ArchiveMark, ArchiveRow, ArchiveTable},
    backend::{RowsAffected, SqlBackend},
    common::{
        is_trigger_in_blackout, json_metadata, json_opt_i64, json_opt_str, json_opt_uuid, json_str,
        next_execution_for_cron, status_list, trigger_parameters, trigger_state,
        workflow_result_event_type,
    },
    interfaces::DatabaseImpl,
    mappers,
    queries::{self, SqlDialect},
};

const WORKFLOW_RUN_COLUMNS: &str = "id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata";
const WORKFLOW_NODE_RUN_COLUMNS: &str = "id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message, current_executor_replica_id, last_executor_replica_id, executor_claimed_at, executor_released_at";
const REPLICA_COLUMNS: &str = "replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at";
const REPLICA_PROVIDER_COLUMNS: &str = "replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at";

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
        let sql = archive_candidate_sql(table);
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
        let Some(row) = sqlx::query(&self.render(&archive_source_sql(self.dialect(), mark.table)))
            .bind(mark.primary_key)
            .fetch_optional(self.pool())
            .await?
        else {
            return Ok(None);
        };
        let row_json = archive_row_json(mark.table, &row)?;
        Ok(Some(ArchiveRow {
            mark_id: mark.id,
            table: mark.table,
            primary_key: mark.primary_key,
            created_at: mark.created_at,
            row: row_json,
        }))
    }
}

fn archive_candidate_sql(table: ArchiveTable) -> &'static str {
    match table {
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

fn archive_source_sql(dialect: SqlDialect, table: ArchiveTable) -> String {
    match table {
        ArchiveTable::WorkflowRuns => {
            "SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata FROM workflow_runs WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowNodeChunks => {
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE id = ?".to_string()
        }
        ArchiveTable::WorkflowReadyNodes => {
            "SELECT id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at FROM workflow_ready_nodes WHERE id = ? AND completed_at IS NOT NULL".to_string()
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
                key_col = queries::ident(dialect, "key")
            )
        }
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

fn archive_row_json<R>(table: ArchiveTable, row: &R) -> Result<Value, SendableError>
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
    Ok(match table {
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

impl<B> DatabaseImpl for B
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
    async fn run_init_scripts(&self, paths: &[String]) -> Result<(), SendableError> {
        self.init(paths).await
    }

    async fn mark_archive_candidates(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<u64, SendableError> {
        let candidates = self
            .archive_candidate_ids(table, eligible_before, limit.max(1))
            .await?;
        let now = Utc::now().timestamp();
        let archive_day = eligible_before.format("%F").to_string();
        let mut marked = 0;
        for (primary_key, created_at) in candidates {
            let insert = sqlx::query(&self.render(&queries::insert_ignore(
                self.dialect(),
                "archive_marks",
                "id, table_name, primary_key, created_at, eligible_before, archive_day, status, attempts, marked_at",
                "?, ?, ?, ?, ?, ?, 'marked', 0, ?",
                "table_name, primary_key",
                None,
            )))
            .bind(Uuid::now_v7())
            .bind(table.as_str())
            .bind(primary_key.to_string())
            .bind(created_at.timestamp())
            .bind(eligible_before.timestamp())
            .bind(archive_day.as_str())
            .bind(now)
            .execute(self.pool())
            .await?;
            marked += insert.affected();
        }
        Ok(marked)
    }

    async fn claim_archive_marks(
        &self,
        archiver_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ArchiveMark>, SendableError> {
        let columns = "id, table_name, primary_key, created_at, archive_day";
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM archive_marks
                         WHERE status = 'marked'
                           AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                         ORDER BY archive_day, table_name, primary_key
                         LIMIT ?
                     ) AS claimable
                 )",
            ))
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(archiver_id.as_str())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM archive_marks WHERE claimed_by = ? AND claimed_until = ? ORDER BY archive_day, table_name, primary_key",
            )))
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(row_to_archive_mark).collect();
        }

        let sql = self.render(&format!(
            "UPDATE archive_marks
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1
             WHERE id IN (
                 SELECT id FROM archive_marks
                 WHERE status = 'marked'
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY archive_day, table_name, primary_key
                 LIMIT ?{skip}
             )
             RETURNING {columns}",
            skip = queries::skip_locked(self.dialect()),
        ));
        let rows = sqlx::query(&sql)
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(archiver_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(row_to_archive_mark).collect()
    }

    async fn fetch_archive_rows(
        &self,
        marks: Vec<ArchiveMark>,
    ) -> Result<Vec<ArchiveRow>, SendableError> {
        let mut rows = Vec::new();
        for mark in marks {
            if let Some(row) = self.fetch_archive_row(&mark).await? {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    async fn delete_archive_rows(&self, rows: Vec<ArchiveRow>) -> Result<u64, SendableError> {
        let mut deleted = 0;
        for row in rows {
            let sql = format!(
                "DELETE FROM {} WHERE {} = ?",
                row.table.as_str(),
                row.table.primary_key_column()
            );
            let result = sqlx::query(&self.render(&sql))
                .bind(row.primary_key)
                .execute(self.pool())
                .await?;
            deleted += result.affected();
        }
        Ok(deleted)
    }

    async fn complete_archive_marks(&self, mark_ids: Vec<Uuid>) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        let mut updated = 0;
        for mark_id in mark_ids {
            let result = sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET status = 'archived', archived_at = ?, claimed_by = NULL, claimed_until = NULL, last_error = NULL
                 WHERE id = ?",
            ))
            .bind(now)
            .bind(mark_id)
            .execute(self.pool())
            .await?;
            updated += result.affected();
        }
        Ok(updated)
    }

    async fn fail_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
        error: String,
    ) -> Result<u64, SendableError> {
        let mut updated = 0;
        for mark_id in mark_ids {
            let result = sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET claimed_by = NULL, claimed_until = NULL, last_error = ?
                 WHERE id = ? AND status = 'marked'",
            ))
            .bind(error.as_str())
            .bind(mark_id)
            .execute(self.pool())
            .await?;
            updated += result.affected();
        }
        Ok(updated)
    }

    async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        let sql = self.render(&format!(
            "SELECT id, status, parameters, output_json, message, {trigger}, started_at, finished_at, created_at, workflow_run_id, workflow_node_id FROM runs WHERE status = ? ORDER BY created_at, id",
            trigger = queries::ident(self.dialect(), "trigger"),
        ));
        let rows = sqlx::query(&sql)
            .bind(status.as_str())
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_run_summary).collect())
    }

    async fn update_run_status(
        &self,
        run_id: Uuid,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(output_json.map(|v| v.to_string()))
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(run_id),
            )
            .await?;
        Ok(())
    }

    async fn append_run_chunk(
        &self,
        run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> Result<RunChunk, SendableError> {
        let sequence: i64 = sqlx::query(&self.render(
            "SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM run_chunks WHERE run_id = ?",
        ))
        .bind(run_id)
        .fetch_one(self.pool())
        .await?
        .get("next_sequence");
        let columns = "id, run_id, sequence, stream, content, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO run_chunks (id, run_id, sequence, stream, content, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(run_id)
            .bind(sequence)
            .bind(chunk.stream.as_str())
            .bind(chunk.content.as_str())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM run_chunks WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_run_chunk(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO run_chunks (id, run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(run_id)
        .bind(sequence)
        .bind(chunk.stream.as_str())
        .bind(chunk.content.as_str())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_run_chunk(&row))
    }

    async fn fetch_run_chunks(
        &self,
        run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, run_id, sequence, stream, content, created_at FROM run_chunks WHERE run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        ))
        .bind(run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_chunk).collect())
    }

    async fn add_run_artifact(
        &self,
        run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        let columns = "id, run_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO run_artifacts (id, run_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(run_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM run_artifacts WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO run_artifacts (id, run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(run_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_run_artifact(&row))
    }

    async fn fetch_run_artifacts(&self, run_id: Uuid) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_artifact).collect())
    }

    async fn fetch_artifact(
        &self,
        artifact_id: Uuid,
    ) -> Result<Option<RunArtifact>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE id = ?",
        ))
        .bind(artifact_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_run_artifact(&row)))
    }

    async fn fetch_all_artifacts(&self) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_run_artifact).collect())
    }

    async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        let now = Utc::now().timestamp();
        // resolve an existing row by explicit id or by its (namespace, name) identity, else mint a
        // fresh uuid. the namespace branch keeps same-named workflows in different namespaces apart.
        let existing_id = match workflow.id {
            Some(id) => Some(id),
            None => {
                let sql = self.render(match &workflow.namespace {
                    Some(_) => "SELECT id FROM workflows WHERE name = ? AND namespace = ? ORDER BY created_at, id LIMIT 1",
                    None => "SELECT id FROM workflows WHERE name = ? AND namespace IS NULL ORDER BY created_at, id LIMIT 1",
                });
                let mut query = sqlx::query(&sql).bind(workflow.name.as_str());
                if workflow.namespace.is_some() {
                    query = query.bind(workflow.namespace.clone());
                }
                query
                    .fetch_optional(self.pool())
                    .await?
                    .map(|row| row.get::<Uuid, _>("id"))
            }
        };
        let workflow_id = existing_id.unwrap_or_else(Uuid::new_v4);

        // mysql has no usable RETURNING via sqlx: upsert with ON DUPLICATE KEY UPDATE, then read the
        // row back on the same pinned connection by the (now app-generated) id.
        if self.dialect() == SqlDialect::MySql {
            let columns = "id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at";
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "id",
                &[
                    "name",
                    "namespace",
                    "version",
                    "enabled",
                    "input_schema",
                    "definition",
                    "updated_at",
                ],
            );
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO workflows (id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(workflow_id)
            .bind(workflow.name.as_str())
            .bind(workflow.namespace.clone())
            .bind(workflow.version.to_string())
            .bind(workflow.enabled)
            .bind(serde_json::to_string(&workflow.input_type)?)
            .bind(workflow.definition.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM workflows WHERE id = ?")))
                    .bind(workflow_id)
                    .fetch_one(&mut *conn)
                    .await?;
            return Ok(mappers::row_to_workflow(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO workflows (id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, namespace = excluded.namespace, version = excluded.version, enabled = excluded.enabled, input_schema = excluded.input_schema, definition = excluded.definition, updated_at = excluded.updated_at
             RETURNING id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at",
        ))
        .bind(workflow_id)
        .bind(workflow.name.as_str())
        .bind(workflow.namespace.clone())
        .bind(workflow.version.to_string())
        .bind(workflow.enabled)
        .bind(serde_json::to_string(&workflow.input_type)?)
        .bind(workflow.definition.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow(&row))
    }

    async fn insert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        // always insert a brand-new row: unlike upsert_workflow this never resolves an existing id
        // by name, so duplicating a workflow yields a sibling version sharing the same name.
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let columns = "id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at";

        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflows (id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow.name.as_str())
            .bind(workflow.namespace.clone())
            .bind(workflow.version.to_string())
            .bind(workflow.enabled)
            .bind(serde_json::to_string(&workflow.input_type)?)
            .bind(workflow.definition.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM workflows WHERE id = ?")))
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            return Ok(mappers::row_to_workflow(&row));
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflows (id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(workflow.name.as_str())
        .bind(workflow.namespace.clone())
        .bind(workflow.version.to_string())
        .bind(workflow.enabled)
        .bind(serde_json::to_string(&workflow.input_type)?)
        .bind(workflow.definition.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow(&row))
    }

    async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        let rows = sqlx::query("SELECT id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at FROM workflows ORDER BY name")
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow).collect())
    }

    async fn fetch_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = ?"))
            .bind(workflow_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow(&row)))
    }

    async fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        // match either an unqualified `name` or a qualified subflow target `"<namespace>.<name>"`
        // against the stored identity `namespace + "." + name`. matching the concatenation (rather
        // than splitting the target) is unambiguous when a workflow name itself contains dots.
        let concat = if self.dialect() == SqlDialect::MySql {
            "CONCAT(namespace, '.', name)"
        } else {
            "namespace || '.' || name"
        };
        let sql = format!(
            "SELECT id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at \
             FROM workflows WHERE name = ? OR (namespace IS NOT NULL AND {concat} = ?) \
             ORDER BY created_at, id LIMIT 1"
        );
        let row = sqlx::query(&self.render(&sql))
            .bind(&name)
            .bind(&name)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow(&row)))
    }

    async fn delete_workflow(&self, workflow_id: Uuid) -> Result<(), SendableError> {
        // cascade-delete the workflow's runs and every execution record before the workflow row, since
        // workflow_runs.workflow_id is a restrict foreign key. ordered child-to-parent so each delete
        // clears the rows that reference the next table; triggers and their firings cascade with the
        // workflow row itself.
        let run_filter = "workflow_run_id IN (SELECT id FROM workflow_runs WHERE workflow_id = ?)";
        let node_run_filter = "workflow_node_run_id IN (SELECT id FROM workflow_node_runs \
             WHERE workflow_run_id IN (SELECT id FROM workflow_runs WHERE workflow_id = ?))";

        let mut tx = self.pool().begin().await?;
        for sql in [
            format!("DELETE FROM workflow_ready_nodes WHERE {run_filter}"),
            format!("DELETE FROM workflow_orchestration_events WHERE {run_filter}"),
            format!("DELETE FROM workflow_node_chunks WHERE {node_run_filter}"),
            format!("DELETE FROM workflow_node_artifacts WHERE {node_run_filter}"),
            format!("DELETE FROM workflow_result_events WHERE {run_filter}"),
            format!("DELETE FROM workflow_trigger_firings WHERE {run_filter}"),
            "DELETE FROM workflow_node_runs WHERE workflow_run_id IN \
                 (SELECT id FROM workflow_runs WHERE workflow_id = ?)"
                .to_string(),
            "DELETE FROM workflow_runs WHERE workflow_id = ?".to_string(),
            "DELETE FROM workflows WHERE id = ?".to_string(),
        ] {
            sqlx::query(&self.render(&sql))
                .bind(workflow_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger, SendableError> {
        let now = Utc::now().timestamp();
        let trigger_id = trigger.id.unwrap_or_else(Uuid::new_v4);

        // mysql has no usable RETURNING via sqlx: upsert with ON DUPLICATE KEY UPDATE, then read the
        // row back on the same pinned connection by the (now app-generated) id.
        if self.dialect() == SqlDialect::MySql {
            let columns = "id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at";
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "id",
                &[
                    "workflow_id",
                    "kind",
                    "enabled",
                    "configuration",
                    "next_execution",
                    "blackout_start",
                    "blackout_end",
                    "metadata",
                    "updated_at",
                ],
            );
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(&format!(
                "INSERT INTO workflow_triggers (id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(trigger_id)
            .bind(trigger.workflow_id)
            .bind(trigger.kind.as_str())
            .bind(trigger.enabled)
            .bind(trigger.configuration.to_string())
            .bind(trigger.next_execution.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
            .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
            .bind(trigger.metadata.to_string())
            .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_triggers WHERE id = ?"
            )))
            .bind(trigger_id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_trigger(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO workflow_triggers (id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET workflow_id = excluded.workflow_id, kind = excluded.kind, enabled = excluded.enabled, configuration = excluded.configuration, next_execution = excluded.next_execution, blackout_start = excluded.blackout_start, blackout_end = excluded.blackout_end, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at",
        ))
        .bind(trigger_id)
        .bind(trigger.workflow_id)
        .bind(trigger.kind.as_str())
        .bind(trigger.enabled)
        .bind(trigger.configuration.to_string())
        .bind(trigger.next_execution.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
        .bind(trigger.metadata.to_string())
        .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_trigger(&row))
    }

    async fn fetch_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE workflow_id = ? ORDER BY created_at, id"))
            .bind(workflow_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_trigger).collect())
    }

    async fn fetch_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> Result<Option<WorkflowTrigger>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE id = ?"))
            .bind(trigger_id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_workflow_trigger(&row)))
    }

    async fn delete_workflow_trigger(&self, trigger_id: Uuid) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render("DELETE FROM workflow_triggers WHERE id = ?"))
                    .bind(trigger_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let sql = self.render(&format!(
            "SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = {} AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) ORDER BY COALESCE(next_execution, 0), id",
            queries::bool_true(self.dialect()),
        ));
        let rows = sqlx::query(&sql)
            .bind(now.timestamp())
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_trigger).collect())
    }

    async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: Uuid,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?",
                ))
                .bind(next_execution.map(|dt| dt.timestamp()))
                .bind(Utc::now().timestamp())
                .bind(trigger_id),
            )
            .await?;
        Ok(())
    }

    async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let select_sql = self.render(&format!(
            "SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = {} AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) ORDER BY COALESCE(next_execution, 0), id LIMIT ?{}",
            queries::bool_true(self.dialect()),
            queries::skip_locked(self.dialect()),
        ));
        let rows = sqlx::query(&select_sql)
            .bind(now.timestamp())
            .bind(limit.max(1))
            .fetch_all(&mut *tx)
            .await?;

        let firing_sql = self.render(&queries::insert_ignore(
            self.dialect(),
            "workflow_trigger_firings",
            "id, trigger_id, fire_key, scheduler_id, created_at",
            "?, ?, ?, ?, ?",
            "trigger_id, fire_key",
            None,
        ));
        let update_next_sql = self
            .render("UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?");

        let mut runs = Vec::new();
        for row in rows {
            let mut trigger = mappers::row_to_workflow_trigger(&row);
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let cron_schedule = trigger
                .configuration
                .get("cron")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if trigger.next_execution.is_none() {
                trigger.next_execution = Some(next_execution_for_cron(cron_schedule, now)?);
                sqlx::query(&update_next_sql)
                    .bind(trigger.next_execution.map(|dt| dt.timestamp()))
                    .bind(now.timestamp())
                    .bind(trigger_id)
                    .execute(&mut *tx)
                    .await?;
                continue;
            }

            if is_trigger_in_blackout(&trigger, now) {
                if let Some(end) = trigger.blackout_end {
                    sqlx::query(&update_next_sql)
                        .bind(end.timestamp())
                        .bind(now.timestamp())
                        .bind(trigger_id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let fire_key = trigger
                .next_execution
                .map(|dt| dt.timestamp().to_string())
                .unwrap_or_else(|| "initial".into());
            let insert = sqlx::query(&firing_sql)
                .bind(Uuid::now_v7())
                .bind(trigger_id)
                .bind(fire_key.as_str())
                .bind(scheduler_id.as_str())
                .bind(now.timestamp())
                .execute(&mut *tx)
                .await?;
            if insert.affected() == 0 {
                continue;
            }

            let workflow_row = sqlx::query(&self.render("SELECT id, name, namespace, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = ?"))
                .bind(trigger.workflow_id)
                .fetch_one(&mut *tx)
                .await?;
            let workflow_snapshot = mappers::row_to_workflow(&workflow_row);
            let new_run_id = Uuid::now_v7();
            let run_row = if self.dialect() == SqlDialect::MySql {
                sqlx::query(&self.render(
                    "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, ?)",
                ))
                .bind(new_run_id)
                .bind(trigger.workflow_id)
                .bind(serde_json::to_string(&workflow_snapshot)?)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(trigger_parameters(&trigger).to_string())
                .bind(trigger_state(&trigger).to_string())
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id.as_str())
                .bind(trigger.metadata.to_string())
                .execute(&mut *tx)
                .await?;
                sqlx::query(&self.render(&format!(
                    "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
                )))
                .bind(new_run_id)
                .fetch_one(&mut *tx)
                .await?
            } else {
                sqlx::query(&self.render(&format!(
                    "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata)
                     VALUES (?, ?, ?, ?, NULL, ?, ?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, ?)
                     RETURNING {WORKFLOW_RUN_COLUMNS}",
                )))
                .bind(new_run_id)
                .bind(trigger.workflow_id)
                .bind(serde_json::to_string(&workflow_snapshot)?)
                .bind(WorkflowStatus::Queued.as_str())
                .bind(trigger_parameters(&trigger).to_string())
                .bind(trigger_state(&trigger).to_string())
                .bind(now.timestamp())
                .bind("cron")
                .bind("replica")
                .bind(scheduler_id.as_str())
                .bind(trigger.metadata.to_string())
                .fetch_one(&mut *tx)
                .await?
            };
            let run = mappers::row_to_workflow_run(&run_row);

            sqlx::query(&self.render("UPDATE workflow_trigger_firings SET workflow_run_id = ? WHERE trigger_id = ? AND fire_key = ?"))
                .bind(run.id)
                .bind(trigger_id)
                .bind(fire_key.as_str())
                .execute(&mut *tx)
                .await?;

            let next_execution = next_execution_for_cron(cron_schedule, now)?;
            sqlx::query(&update_next_sql)
                .bind(next_execution.timestamp())
                .bind(now.timestamp())
                .bind(trigger_id)
                .execute(&mut *tx)
                .await?;
            runs.push(run);
        }

        tx.commit().await?;
        Ok(runs)
    }

    async fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> Result<WorkflowRun, SendableError> {
        let snapshot = serde_json::to_string(&workflow_snapshot)?;
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_id)
            .bind(snapshot)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(parameters.to_string())
            .bind(state.to_string())
            .bind(created_at)
            .bind(name)
            .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
            .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
            .bind(provenance.actor_replica_id)
            .bind(provenance.actor_display_name)
            .bind(provenance.request_host)
            .bind(provenance.request_ip)
            .bind(provenance.metadata.to_string())
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_run(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_runs (id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata)
             VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {WORKFLOW_RUN_COLUMNS}",
        )))
        .bind(id)
        .bind(workflow_id)
        .bind(snapshot)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(state.to_string())
        .bind(created_at)
        .bind(name)
        .bind(provenance.source_kind.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_type.map(|value| value.as_str().to_string()))
        .bind(provenance.actor_replica_id)
        .bind(provenance.actor_display_name)
        .bind(provenance.request_host)
        .bind(provenance.request_ip)
        .bind(provenance.metadata.to_string())
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_run(&row))
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE id = ?"
        )))
        .bind(workflow_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_workflow_run(&row)))
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE status = ? ORDER BY created_at, id"
        )))
        .bind(status.as_str())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let statuses = status_list(&statuses);
        // mysql has no UPDATE ... RETURNING and forbids a subquery on the table being updated, so
        // claim with a derived-table subselect, then read the rows back by the lease just written.
        if self.dialect() == SqlDialect::MySql {
            let claim_sql = self.render(&format!(
                "UPDATE workflow_runs SET scheduler_claimed_by = ?, scheduler_claimed_until = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_runs
                         WHERE status IN ({statuses})
                           AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= ? OR scheduler_claimed_by = ?)
                         ORDER BY created_at, id
                         LIMIT ?
                     ) AS claimable
                 )",
            ));
            sqlx::query(&claim_sql)
                .bind(scheduler_id.as_str())
                .bind(lease_until.timestamp())
                .bind(now.timestamp())
                .bind(scheduler_id.as_str())
                .bind(limit.max(1))
                .execute(self.pool())
                .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE scheduler_claimed_by = ? AND scheduler_claimed_until = ? ORDER BY created_at, id",
            )))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return Ok(rows.iter().map(mappers::row_to_workflow_run).collect());
        }

        let sql = self.render(&format!(
            "UPDATE workflow_runs SET scheduler_claimed_by = ?, scheduler_claimed_until = ?
             WHERE id IN (
                 SELECT id FROM workflow_runs
                 WHERE status IN ({statuses})
                   AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= ? OR scheduler_claimed_by = ?)
                 ORDER BY created_at, id
                 LIMIT ?{skip}
             )
             RETURNING {WORKFLOW_RUN_COLUMNS}",
            skip = queries::skip_locked(self.dialect()),
        ));
        let rows = sqlx::query(&sql)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE workflow_runs SET scheduler_claimed_until = ? WHERE id = ? AND scheduler_claimed_by = ?",
        ))
        .bind(lease_until.timestamp())
        .bind(workflow_run_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn release_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET scheduler_claimed_by = NULL, scheduler_claimed_until = NULL WHERE id = ? AND scheduler_claimed_by = ?",
                ))
                .bind(workflow_run_id)
                .bind(scheduler_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_recent_workflow_runs(&self) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs ORDER BY created_at DESC, id DESC"
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE workflow_id = ? ORDER BY created_at DESC, id DESC")))
            .bind(workflow_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = if open_only {
            sqlx::query(&self.render(&format!("SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE name = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled') ORDER BY created_at DESC, id DESC")))
                .bind(name)
                .fetch_all(self.pool())
                .await?
        } else {
            sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_RUN_COLUMNS} FROM workflow_runs WHERE name = ? ORDER BY created_at DESC, id DESC"
            )))
            .bind(name)
            .fetch_all(self.pool())
            .await?
        };
        Ok(rows.iter().map(mappers::row_to_workflow_run).collect())
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render("UPDATE workflow_runs SET name = ? WHERE id = ?"))
                    .bind(name)
                    .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn update_workflow_run_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = COALESCE(?, state), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(active_node_id)
                .bind(state.map(|value| value.to_string()))
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(terminal)
                .bind(now)
                .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let empty_state = Value::Object(Default::default()).to_string();
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_runs (id, workflow_run_id, node_id, status, attempt, parameters, state, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_run_id)
            .bind(node_id)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(0i64)
            .bind(parameters.to_string())
            .bind(empty_state)
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_runs (id, workflow_run_id, node_id, status, attempt, parameters, state, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {WORKFLOW_NODE_RUN_COLUMNS}",
        )))
        .bind(id)
        .bind(workflow_run_id)
        .bind(node_id)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(0i64)
        .bind(parameters.to_string())
        .bind(empty_state)
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run(&row))
    }

    async fn update_workflow_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs SET status = ?, attempt = COALESCE(?, attempt), parameters = COALESCE(?, parameters), output_json = COALESCE(?, output_json), state = COALESCE(?, state), transition_reason = COALESCE(?, transition_reason), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ?",
                ))
                .bind(status.as_str())
                .bind(attempt)
                .bind(parameters.map(|value| value.to_string()))
                .bind(output_json.map(|value| value.to_string()))
                .bind(state.map(|value| value.to_string()))
                .bind(transition_reason)
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(status.as_str())
                .bind(terminal)
                .bind(now)
                .bind(status.as_str())
                .bind(node_run_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE workflow_run_id = ? ORDER BY created_at, id")))
            .bind(workflow_run_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_node_run).collect())
    }

    async fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Option<WorkflowNodeRun>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE id = ?"
        )))
        .bind(workflow_node_run_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_workflow_node_run(&row)))
    }

    async fn fetch_workflow_node_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {WORKFLOW_NODE_RUN_COLUMNS} FROM workflow_node_runs WHERE status = ? ORDER BY created_at, id"
        )))
        .bind(status.as_str())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_workflow_node_run).collect())
    }

    async fn claim_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        // compare-and-swap lease: only acquire when no live executor holds the slot. a redelivered
        // or timeout-raced duplicate of the same node run thus cannot execute concurrently; the slot
        // frees on release or once the prior claim ages past `stale_before` (the caller's deadline).
        let result = self
            .pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs
                     SET current_executor_replica_id = ?, executor_claimed_at = ?, executor_released_at = NULL
                     WHERE id = ?
                       AND (current_executor_replica_id IS NULL OR executor_claimed_at < ?)",
                ))
                .bind(replica_id)
                .bind(claimed_at.timestamp())
                .bind(node_run_id)
                .bind(stale_before.timestamp()),
            )
            .await?;
        Ok(result.affected() > 0)
    }

    async fn release_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        released_at: DateTime<Utc>,
    ) -> Result<(), SendableError> {
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs
                     SET last_executor_replica_id = ?, current_executor_replica_id = NULL, executor_released_at = ?
                     WHERE id = ?",
                ))
                .bind(replica_id)
                .bind(released_at.timestamp())
                .bind(node_run_id),
            )
            .await?;
        Ok(())
    }

    async fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> Result<WorkflowNodeRunChunk, SendableError> {
        let sequence: i64 = sqlx::query(&self.render("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?"))
            .bind(workflow_node_run_id)
            .fetch_one(self.pool())
            .await?
            .get("next_sequence");
        let columns = "id, workflow_node_run_id, sequence, stream, content, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_node_run_id)
            .bind(sequence)
            .bind(chunk.stream.as_str())
            .bind(chunk.content.as_str())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_node_chunks WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run_chunk(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(workflow_node_run_id)
        .bind(sequence)
        .bind(chunk.stream.as_str())
        .bind(chunk.content.as_str())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run_chunk(&row))
    }

    async fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE workflow_node_run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        ))
        .bind(workflow_node_run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_chunk)
            .collect())
    }

    async fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> Result<WorkflowNodeRunArtifact, SendableError> {
        let columns =
            "id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(workflow_node_run_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_node_artifacts WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_node_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(workflow_node_run_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_node_run_artifact(&row))
    }

    async fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_node_artifacts WHERE workflow_node_run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(workflow_node_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn fetch_workflow_node_run_artifacts_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT a.id, a.workflow_node_run_id, a.name, a.mime_type, a.size_bytes, a.uri, a.metadata, a.created_at
             FROM workflow_node_artifacts a
             JOIN workflow_node_runs r ON a.workflow_node_run_id = r.id
             WHERE r.workflow_run_id = ?
             ORDER BY a.created_at ASC, a.id ASC",
        ))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn add_workflow_run_artifact(
        &self,
        artifact: &NewWorkflowRunArtifact,
    ) -> Result<WorkflowRunArtifact, SendableError> {
        let columns = "id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO workflow_run_artifacts (id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(artifact.workflow_run_id)
            .bind(artifact.node_id.as_str())
            .bind(artifact.artifact_id)
            .bind(artifact.name.as_str())
            .bind(artifact.mime_type.as_str())
            .bind(artifact.size_bytes)
            .bind(artifact.uri.as_str())
            .bind(artifact.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_run_artifacts WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_workflow_run_artifact(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_run_artifacts (id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(artifact.workflow_run_id)
        .bind(artifact.node_id.as_str())
        .bind(artifact.artifact_id)
        .bind(artifact.name.as_str())
        .bind(artifact.mime_type.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_workflow_run_artifact(&row))
    }

    async fn fetch_workflow_run_artifacts(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowRunArtifact>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, workflow_run_id, node_id, artifact_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_run_artifacts WHERE workflow_run_id = ? ORDER BY created_at ASC, id ASC",
        ))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_workflow_run_artifact)
            .collect())
    }

    async fn apply_workflow_result_event(
        &self,
        event: &WorkflowResultEvent,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        let event_type = workflow_result_event_type(event);
        let insert = sqlx::query(&self.render(&queries::insert_ignore(
            self.dialect(),
            "workflow_result_events",
            "event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, created_at",
            "?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        )))
        .bind(event.event_id)
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(event.node_id.clone())
        .bind(event_type)
        .bind(event.timestamp.timestamp())
        .execute(&mut *tx)
        .await?;

        if insert.affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }

        match &event.kind {
            WorkflowResultEventKind::Status {
                status,
                output_json,
                message,
            } => {
                let now = Utc::now().timestamp();
                let terminal = status.is_terminal();
                sqlx::query(&self.render(
                    "UPDATE workflow_node_runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ? AND NOT (status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND ? NOT IN ('succeeded', 'failed', 'timed_out', 'canceled'))",
                ))
                .bind(status.as_str())
                .bind(output_json.as_ref().map(|value: &Value| value.to_string()))
                .bind(message.clone())
                .bind(status.as_str())
                .bind(now)
                .bind(status.as_str())
                .bind(terminal)
                .bind(now)
                .bind(status.as_str())
                .bind(event.workflow_node_run_id)
                .bind(status.as_str())
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Chunk { chunk } => {
                let sequence: i64 = sqlx::query(&self.render("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?"))
                    .bind(event.workflow_node_run_id)
                    .fetch_one(&mut *tx)
                    .await?
                    .get("next_sequence");
                sqlx::query(&self.render(
                    "INSERT INTO workflow_node_chunks (id, workflow_node_run_id, sequence, stream, content, created_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                ))
                .bind(Uuid::now_v7())
                .bind(event.workflow_node_run_id)
                .bind(sequence)
                .bind(chunk.stream.as_str())
                .bind(chunk.content.as_str())
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Artifact { artifact } => {
                sqlx::query(&self.render(
                    "INSERT INTO workflow_node_artifacts (id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                ))
                .bind(Uuid::now_v7())
                .bind(event.workflow_node_run_id)
                .bind(artifact.name.as_str())
                .bind(artifact.mime_type.as_str())
                .bind(artifact.size_bytes)
                .bind(artifact.uri.as_str())
                .bind(artifact.metadata.to_string())
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(true)
    }

    async fn append_orchestration_event(
        &self,
        event: &NewOrchestrationEvent,
    ) -> Result<bool, SendableError> {
        let insert = sqlx::query(&self.render(&queries::insert_ignore(
            self.dialect(),
            "workflow_orchestration_events",
            "event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at",
            "?, ?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        )))
        .bind(event.event_id)
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(event.node_id.clone())
        .bind(event.event_type.as_str())
        .bind(event.payload.to_string())
        .bind(event.created_at.timestamp())
        .execute(self.pool())
        .await?;
        Ok(insert.affected() > 0)
    }

    async fn fetch_orchestration_events(
        &self,
        workflow_run_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OrchestrationEvent>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at
             FROM workflow_orchestration_events
             WHERE workflow_run_id = ?
             ORDER BY created_at, event_id
             LIMIT ?",
        ))
        .bind(workflow_run_id)
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_orchestration_event)
            .collect()
    }

    async fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let inserted_event = sqlx::query(&self.render(&queries::insert_ignore(
            self.dialect(),
            "workflow_orchestration_events",
            "event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at",
            "?, ?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        )))
        .bind(event.event_id)
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(event.node_id.clone())
        .bind(event.event_type.as_str())
        .bind(event.payload.to_string())
        .bind(event.created_at.timestamp())
        .execute(&mut *tx)
        .await?;
        if inserted_event.affected() == 0 {
            tx.commit().await?;
            return Ok(None);
        }

        let now = Utc::now().timestamp();
        let ready_id = Uuid::now_v7();
        let ready_columns = "id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at";

        // mysql has no RETURNING on INSERT IGNORE, so insert then read the row back on the same tx.
        let row = if self.dialect() == SqlDialect::MySql {
            let inserted = sqlx::query(&self.render(&queries::insert_ignore(
                SqlDialect::MySql,
                "workflow_ready_nodes",
                "id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, created_at, updated_at",
                "?, ?, ?, ?, 'queued', ?, 0, ?, ?",
                "source_event_id, workflow_run_id, node_id",
                None,
            )))
            .bind(ready_id)
            .bind(event.event_id)
            .bind(event.workflow_run_id)
            .bind(node_id.as_str())
            .bind(ready_at.timestamp())
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if inserted.affected() == 0 {
                None
            } else {
                Some(
                    sqlx::query(&self.render(&format!(
                        "SELECT {ready_columns} FROM workflow_ready_nodes WHERE source_event_id = ? AND workflow_run_id = ? AND node_id = ?",
                    )))
                    .bind(event.event_id)
                    .bind(event.workflow_run_id)
                    .bind(node_id.as_str())
                    .fetch_one(&mut *tx)
                    .await?,
                )
            }
        } else {
            sqlx::query(&self.render(&queries::insert_ignore(
                self.dialect(),
                "workflow_ready_nodes",
                "id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, created_at, updated_at",
                "?, ?, ?, ?, 'queued', ?, 0, ?, ?",
                "source_event_id, workflow_run_id, node_id",
                Some(ready_columns),
            )))
            .bind(ready_id)
            .bind(event.event_id)
            .bind(event.workflow_run_id)
            .bind(node_id.as_str())
            .bind(ready_at.timestamp())
            .bind(now)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?
        };
        tx.commit().await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
    }

    async fn claim_ready_nodes(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let columns = "id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at";

        // mysql has no UPDATE ... RETURNING and cannot subquery the table being updated, so claim
        // via a derived-table subselect, then read the claimed rows back by the lease just written.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE workflow_ready_nodes
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM workflow_ready_nodes
                         WHERE completed_at IS NULL
                           AND ready_at <= ?
                           AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                         ORDER BY ready_at, id
                         LIMIT ?
                     ) AS claimable
                 )",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_ready_nodes WHERE claimed_by = ? AND claimed_until = ? ORDER BY ready_at, id",
            )))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(mappers::row_to_ready_node).collect();
        }

        let sql = self.render(&format!(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_ready_nodes
                 WHERE completed_at IS NULL
                   AND ready_at <= ?
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY ready_at, id
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
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(mappers::row_to_ready_node).collect()
    }

    async fn fetch_ready_node(
        &self,
        ready_node_id: Uuid,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at
             FROM workflow_ready_nodes
             WHERE id = ?",
        ))
        .bind(ready_node_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
    }

    async fn complete_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE workflow_ready_nodes
             SET completed_at = ?, status = 'succeeded', updated_at = ?
             WHERE id = ? AND claimed_by = ?",
        ))
        .bind(now)
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at
             FROM workflow_ready_nodes
             WHERE completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ?)
             ORDER BY ready_at, id
             LIMIT ?",
        ))
        .bind(now.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_ready_node).collect()
    }

    async fn claim_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let columns = "id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at";

        // mysql has no UPDATE ... RETURNING: claim by id, then read back only if we hold the lease.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE workflow_ready_nodes
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
                 WHERE id = ?
                   AND completed_at IS NULL
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)",
            ))
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(ready_node_id)
            .bind(now.timestamp())
            .bind(scheduler_id.as_str())
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM workflow_ready_nodes WHERE id = ? AND claimed_by = ? AND claimed_until = ?",
            )))
            .bind(ready_node_id)
            .bind(scheduler_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_optional(self.pool())
            .await?;
            return row.as_ref().map(mappers::row_to_ready_node).transpose();
        }

        let row = sqlx::query(&self.render(&format!(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id = ?
               AND completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
             RETURNING {columns}",
        )))
        .bind(scheduler_id.as_str())
        .bind(lease_until.timestamp())
        .bind(now.timestamp())
        .bind(ready_node_id)
        .bind(now.timestamp())
        .bind(scheduler_id.as_str())
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_ready_node).transpose()
    }

    async fn release_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE workflow_ready_nodes
             SET claimed_by = NULL, claimed_until = NULL, status = 'queued', updated_at = ?
             WHERE id = ? AND claimed_by = ? AND completed_at IS NULL",
        ))
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn upsert_catalog_item(&self, item: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        // catalog_items.id is a uuid primary key with no db default; generate one for the insert
        // path. on a uri conflict the update set never touches id, so existing rows keep theirs.
        let id = Uuid::new_v4();
        let columns =
            "id, uri, item_type, name, version, document, metadata, created_at, updated_at";
        let document = item
            .get("document")
            .cloned()
            .unwrap_or(Value::Object(Default::default()))
            .to_string();

        if self.dialect() == SqlDialect::MySql {
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "uri",
                &[
                    "item_type",
                    "name",
                    "version",
                    "document",
                    "metadata",
                    "updated_at",
                ],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO catalog_items (id, uri, item_type, name, version, document, metadata, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(id)
            .bind(json_str(&item, "uri"))
            .bind(json_str(&item, "item_type"))
            .bind(json_str(&item, "name"))
            .bind(json_str(&item, "version"))
            .bind(document)
            .bind(json_metadata(&item))
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM catalog_items WHERE uri = ?",
            )))
            .bind(json_str(&item, "uri"))
            .fetch_one(self.pool())
            .await?;
            return Ok(mappers::row_to_catalog_item(&row));
        }

        let row = sqlx::query(&self.render(
            "INSERT INTO catalog_items (id, uri, item_type, name, version, document, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(uri) DO UPDATE SET item_type = excluded.item_type, name = excluded.name, version = excluded.version, document = excluded.document, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, uri, item_type, name, version, document, metadata, created_at, updated_at",
        ))
        .bind(id)
        .bind(json_str(&item, "uri"))
        .bind(json_str(&item, "item_type"))
        .bind(json_str(&item, "name"))
        .bind(json_str(&item, "version"))
        .bind(document)
        .bind(json_metadata(&item))
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_catalog_item(&row))
    }

    async fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = if let Some(item_type) = item_type {
            sqlx::query(&self.render("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE item_type = ? ORDER BY uri"))
                .bind(item_type)
                .fetch_all(self.pool())
                .await?
        } else {
            sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items ORDER BY uri")
                .fetch_all(self.pool())
                .await?
        };
        Ok(rows.iter().map(mappers::row_to_catalog_item).collect())
    }

    async fn fetch_catalog_item(&self, uri: String) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE uri = ?"))
            .bind(uri)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|row| mappers::row_to_catalog_item(&row)))
    }

    async fn register_replica(
        &self,
        request: ReplicaRegistrationRequest,
        observed_ip: Option<String>,
    ) -> Result<ReplicaRecord, SendableError> {
        let now = Utc::now().timestamp();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'stale' WHERE instance_id = ? AND runtime_id <> ? AND status = 'live'",
                ))
                .bind(request.instance_id.as_str())
                .bind(request.runtime_id.as_str()),
            )
            .await?;
        let replica_id = Uuid::now_v7();
        if self.dialect() == SqlDialect::MySql {
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "instance_id, runtime_id",
                &[
                    "replica_type",
                    "status",
                    "display_name",
                    "host",
                    "port",
                    "base_path",
                    "observed_ip",
                    "version",
                    "attributes",
                    "last_heartbeat_at",
                    "last_seen_at",
                    "offline_at",
                ],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO replicas (replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at)
                 VALUES (?, ?, ?, ?, 'live', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL) {conflict}",
            )))
            .bind(replica_id)
            .bind(request.replica_type.as_str())
            .bind(request.instance_id.as_str())
            .bind(request.runtime_id.as_str())
            .bind(request.display_name.clone())
            .bind(request.host.clone())
            .bind(request.port.map(i64::from))
            .bind(request.base_path.clone())
            .bind(observed_ip.clone())
            .bind(request.version.clone())
            .bind(request.attributes.to_string())
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {REPLICA_COLUMNS} FROM replicas WHERE instance_id = ? AND runtime_id = ?",
            )))
            .bind(request.instance_id)
            .bind(request.runtime_id)
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_replica(&row);
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO replicas (replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at)
             VALUES (?, ?, ?, ?, 'live', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
             ON CONFLICT(instance_id, runtime_id) DO UPDATE SET replica_type = excluded.replica_type, status = 'live', display_name = excluded.display_name, host = excluded.host, port = excluded.port, base_path = excluded.base_path, observed_ip = excluded.observed_ip, version = excluded.version, attributes = excluded.attributes, last_heartbeat_at = excluded.last_heartbeat_at, last_seen_at = excluded.last_seen_at, offline_at = NULL
             RETURNING {REPLICA_COLUMNS}",
        )))
        .bind(replica_id)
        .bind(request.replica_type.as_str())
        .bind(request.instance_id)
        .bind(request.runtime_id)
        .bind(request.display_name)
        .bind(request.host)
        .bind(request.port.map(i64::from))
        .bind(request.base_path)
        .bind(observed_ip)
        .bind(request.version)
        .bind(request.attributes.to_string())
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_replica(&row)
    }

    async fn heartbeat_replica(
        &self,
        replica_id: Uuid,
        request: ReplicaHeartbeatRequest,
        observed_ip: Option<String>,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let now = Utc::now().timestamp();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'live', display_name = COALESCE(?, display_name), host = COALESCE(?, host), port = COALESCE(?, port), base_path = COALESCE(?, base_path), observed_ip = COALESCE(?, observed_ip), attributes = COALESCE(?, attributes), last_heartbeat_at = ?, last_seen_at = ?, offline_at = NULL
                     WHERE replica_id = ? AND runtime_id = ?",
                ))
                .bind(request.display_name.clone())
                .bind(request.host.clone())
                .bind(request.port.map(i64::from))
                .bind(request.base_path.clone())
                .bind(observed_ip.clone())
                .bind(Some(request.attributes.to_string()))
                .bind(now)
                .bind(now)
                .bind(replica_id)
                .bind(request.runtime_id.as_str()),
            )
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE replica_id = ? AND runtime_id = ?",
        )))
        .bind(replica_id)
        .bind(request.runtime_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn mark_replica_offline(
        &self,
        replica_id: Uuid,
        runtime_id: String,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let now = Utc::now().timestamp();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'offline', offline_at = ?, last_seen_at = ? WHERE replica_id = ? AND runtime_id = ?",
                ))
                .bind(now)
                .bind(now)
                .bind(replica_id)
                .bind(runtime_id.as_str()),
            )
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE replica_id = ? AND runtime_id = ?",
        )))
        .bind(replica_id)
        .bind(runtime_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn reap_inactive_replicas(&self, cutoff: DateTime<Utc>) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE replicas SET status = 'offline', offline_at = ? WHERE last_heartbeat_at <= ? AND status <> 'offline'",
        ))
        .bind(now)
        .bind(cutoff.timestamp())
        .execute(self.pool())
        .await?;
        Ok(result.affected())
    }

    async fn delete_expired_replicas(&self, cutoff: DateTime<Utc>) -> Result<u64, SendableError> {
        // null the historical attribution pointers (restrict-mode foreign keys) before deleting so
        // the delete does not error; provider registrations cascade. a replica still claimed as a node
        // run's current executor is excluded from the delete and left until that run resolves.
        let cutoff_ts = cutoff.timestamp();
        let mut tx = self.pool().begin().await?;

        sqlx::query(&self.render(
            "UPDATE workflow_runs SET trigger_actor_replica_id = NULL
             WHERE trigger_actor_replica_id IN
                 (SELECT replica_id FROM replicas WHERE last_heartbeat_at <= ?)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        sqlx::query(&self.render(
            "UPDATE workflow_node_runs SET last_executor_replica_id = NULL
             WHERE last_executor_replica_id IN
                 (SELECT replica_id FROM replicas WHERE last_heartbeat_at <= ?)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        let deleted = sqlx::query(&self.render(
            "DELETE FROM replicas WHERE last_heartbeat_at <= ? AND replica_id NOT IN
                 (SELECT current_executor_replica_id FROM workflow_node_runs
                  WHERE current_executor_replica_id IS NOT NULL)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(deleted.affected())
    }

    async fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> Result<Vec<ReplicaRecord>, SendableError> {
        let rows = if let Some(replica_type) = replica_type {
            sqlx::query(&self.render(&format!(
                "SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at
                 FROM replicas WHERE replica_type = ? ORDER BY replica_type, instance_id, replica_id"
            )))
            .bind(stale_before.timestamp())
            .bind(replica_type.as_str())
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(&self.render(
                "SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at
                 FROM replicas ORDER BY replica_type, instance_id, replica_id",
            ))
            .bind(stale_before.timestamp())
            .fetch_all(self.pool())
            .await?
        };
        let mut replicas = rows
            .iter()
            .map(mappers::row_to_replica)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(status) = status {
            replicas.retain(|replica| replica.status == status);
        }
        Ok(replicas)
    }

    async fn upsert_replica_provider_registration(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> Result<ReplicaProviderRegistration, SendableError> {
        let now = Utc::now().timestamp();
        let provider_json = serde_json::to_string(&request.provider)?;
        if self.dialect() == SqlDialect::MySql {
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "replica_id, provider_name",
                &["provider_json", "last_registered_at", "last_heartbeat_at"],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO replica_provider_registrations (replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at)
                 VALUES (?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(replica_id)
            .bind(request.provider.name.as_str())
            .bind(provider_json)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {REPLICA_PROVIDER_COLUMNS} FROM replica_provider_registrations WHERE replica_id = ? AND provider_name = ?",
            )))
            .bind(replica_id)
            .bind(request.provider.name.as_str())
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_replica_provider_registration(&row);
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO replica_provider_registrations (replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(replica_id, provider_name) DO UPDATE SET provider_json = excluded.provider_json, last_registered_at = excluded.last_registered_at, last_heartbeat_at = excluded.last_heartbeat_at
             RETURNING {REPLICA_PROVIDER_COLUMNS}",
        )))
        .bind(replica_id)
        .bind(request.provider.name.as_str())
        .bind(provider_json)
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_replica_provider_registration(&row)
    }

    async fn fetch_replica_provider_registrations(
        &self,
        replica_id: Uuid,
    ) -> Result<Vec<ReplicaProviderRegistration>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_PROVIDER_COLUMNS} FROM replica_provider_registrations WHERE replica_id = ? ORDER BY provider_name"
        )))
        .bind(replica_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_replica_provider_registration)
            .collect()
    }

    async fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let columns = "id, record_type, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO automation_records (id, record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(record_type)
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_opt_uuid(&record, "external_item_id"))
            .bind(json_opt_str(&record, "node_id"))
            .bind(json_str(&record, "provider"))
            .bind(json_str(&record, "resource_type"))
            .bind(json_str(&record, "external_id"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "title"))
            .bind(json_opt_str(&record, "url"))
            .bind(json_opt_str(&record, "body"))
            .bind(json_opt_str(&record, "path"))
            .bind(json_opt_str(&record, "prompt"))
            .bind(json_opt_str(&record, "approval_type"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM automation_records WHERE id = ?"
            )))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_automation_record(&row));
        }
        let row = sqlx::query(&self.render(
            "INSERT INTO automation_records (id, record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, record_type, data, created_at, updated_at",
        ))
        .bind(id)
        .bind(record_type)
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_opt_uuid(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_automation_record(&row))
    }

    async fn update_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let columns = "id, record_type, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE automation_records SET workflow_run_id = ?, external_item_id = ?, node_id = ?, provider = ?, resource_type = ?, external_id = ?, status = ?, title = ?, url = ?, body = ?, path = ?, prompt = ?, approval_type = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? AND record_type = ?",
            ))
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_opt_uuid(&record, "external_item_id"))
            .bind(json_opt_str(&record, "node_id"))
            .bind(json_str(&record, "provider"))
            .bind(json_str(&record, "resource_type"))
            .bind(json_str(&record, "external_id"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "title"))
            .bind(json_opt_str(&record, "url"))
            .bind(json_opt_str(&record, "body"))
            .bind(json_opt_str(&record, "path"))
            .bind(json_opt_str(&record, "prompt"))
            .bind(json_opt_str(&record, "approval_type"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(record_id)
            .bind(record_type.as_str())
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM automation_records WHERE id = ? AND record_type = ?",
            )))
            .bind(record_id)
            .bind(record_type)
            .fetch_one(self.pool())
            .await?;
            return Ok(mappers::row_to_automation_record(&row));
        }
        let row = sqlx::query(&self.render(
            "UPDATE automation_records SET workflow_run_id = ?, external_item_id = ?, node_id = ?, provider = ?, resource_type = ?, external_id = ?, status = ?, title = ?, url = ?, body = ?, path = ?, prompt = ?, approval_type = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? AND record_type = ? RETURNING id, record_type, data, created_at, updated_at",
        ))
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_opt_uuid(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(record_id)
        .bind(record_type)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_automation_record(&row))
    }

    async fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<Uuid>,
        external_item_id: Option<Uuid>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE record_type = ? ORDER BY created_at DESC, id DESC"))
            .bind(record_type)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_automation_record)
            .filter(|record| {
                workflow_run_id.is_none_or(|id| {
                    record.get("workflow_run_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                }) && external_item_id.is_none_or(|id| {
                    record.get("external_item_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                })
            })
            .collect())
    }

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

    async fn create_gate(&self, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let columns = "id, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO gates (id, workflow_run_id, node_id, kind, status, label, reason, resolved_by, resolved_at, metadata, data, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(json_opt_uuid(&record, "workflow_run_id"))
            .bind(json_str(&record, "node_id"))
            .bind(json_str(&record, "kind"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "label"))
            .bind(json_opt_str(&record, "reason"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM gates WHERE id = ?")))
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            return Ok(mappers::row_to_gate(&row));
        }
        let row = sqlx::query(&self.render(
            "INSERT INTO gates (id, workflow_run_id, node_id, kind, status, label, reason, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, data, created_at, updated_at",
        ))
        .bind(id)
        .bind(json_opt_uuid(&record, "workflow_run_id"))
        .bind(json_str(&record, "node_id"))
        .bind(json_str(&record, "kind"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "label"))
        .bind(json_opt_str(&record, "reason"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_gate(&row))
    }

    async fn update_gate(&self, gate_id: Uuid, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let columns = "id, data, created_at, updated_at";
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "UPDATE gates SET node_id = ?, kind = ?, status = ?, label = ?, reason = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ?",
            ))
            .bind(json_str(&record, "node_id"))
            .bind(json_str(&record, "kind"))
            .bind(json_str(&record, "status"))
            .bind(json_opt_str(&record, "label"))
            .bind(json_opt_str(&record, "reason"))
            .bind(json_opt_str(&record, "resolved_by"))
            .bind(json_opt_i64(&record, "resolved_at"))
            .bind(json_metadata(&record))
            .bind(record.to_string())
            .bind(now)
            .bind(gate_id)
            .execute(self.pool())
            .await?;
            let row =
                sqlx::query(&self.render(&format!("SELECT {columns} FROM gates WHERE id = ?")))
                    .bind(gate_id)
                    .fetch_one(self.pool())
                    .await?;
            return Ok(mappers::row_to_gate(&row));
        }
        let row = sqlx::query(&self.render(
            "UPDATE gates SET node_id = ?, kind = ?, status = ?, label = ?, reason = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? RETURNING id, data, created_at, updated_at",
        ))
        .bind(json_str(&record, "node_id"))
        .bind(json_str(&record, "kind"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "label"))
        .bind(json_opt_str(&record, "reason"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(gate_id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_gate(&row))
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

    async fn fetch_gate(&self, gate_id: Uuid) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query(
            &self.render("SELECT id, data, created_at, updated_at FROM gates WHERE id = ?"),
        )
        .bind(gate_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_gate(&row)))
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

    async fn record_audit_log(&self, record: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        sqlx::query(&self.render(
            "INSERT INTO audit_log (id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(json_opt_uuid(&record, "actor_id"))
        .bind(json_str(&record, "actor_kind"))
        .bind(json_str(&record, "action"))
        .bind(json_opt_str(&record, "resource_type"))
        .bind(json_opt_uuid(&record, "resource_id"))
        .bind(json_str(&record, "outcome"))
        .bind(json_opt_str(&record, "detail"))
        .bind(json_metadata(&record))
        .bind(now)
        .execute(self.pool())
        .await?;
        let row = sqlx::query(&self.render(
            "SELECT id, actor_id, actor_kind, action, resource_type, resource_id, outcome, detail, metadata, created_at FROM audit_log WHERE id = ?",
        ))
        .bind(id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_audit_log(&row))
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

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        let now = Utc::now().timestamp();
        let id = Uuid::now_v7();
        let dispatch_columns = "id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until";

        // first writer wins: keep the existing command on conflict.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(&self.render(
                "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
                 VALUES (?, ?, ?, 0, ?, ?)
                 ON DUPLICATE KEY UPDATE command_json = command_json",
            ))
            .bind(id)
            .bind(dedupe_key.as_str())
            .bind(serde_json::to_string(&command)?)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {dispatch_columns} FROM workflow_action_dispatches WHERE dedupe_key = ?",
            )))
            .bind(dedupe_key)
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_action_dispatch(&row);
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_action_dispatches (id, dedupe_key, command_json, attempts, created_at, updated_at)
             VALUES (?, ?, ?, 0, ?, ?)
             ON CONFLICT(dedupe_key) DO UPDATE SET command_json = workflow_action_dispatches.command_json
             RETURNING {dispatch_columns}",
        )))
        .bind(id)
        .bind(dedupe_key)
        .bind(serde_json::to_string(&command)?)
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_action_dispatch(&row)
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

    async fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let columns = "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";
        let id = Uuid::now_v7();
        let created_at = Utc::now().timestamp();
        if self.dialect() == SqlDialect::MySql {
            let mut conn = self.pool().acquire().await?;
            sqlx::query(&self.render(
                "INSERT INTO notifications (id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(id)
            .bind(notification.workflow_run_id)
            .bind(notification.workflow_node_id.clone())
            .bind(notification.channel.as_str())
            .bind(notification.severity.as_str())
            .bind(notification.title.as_str())
            .bind(notification.body.clone())
            .bind(notification.target.clone())
            .bind(notification.metadata.to_string())
            .bind(created_at)
            .execute(&mut *conn)
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM notifications WHERE id = ?")),
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            return Ok(mappers::row_to_notification(&row));
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO notifications (id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING {columns}",
        )))
        .bind(id)
        .bind(notification.workflow_run_id)
        .bind(notification.workflow_node_id.clone())
        .bind(notification.channel.as_str())
        .bind(notification.severity.as_str())
        .bind(notification.title.as_str())
        .bind(notification.body.clone())
        .bind(notification.target.clone())
        .bind(notification.metadata.to_string())
        .bind(created_at)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_notification(&row))
    }

    async fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        let bounded_limit = limit.clamp(1, 1000);
        let rows = if unread_only {
            sqlx::query(&self.render(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE read_at IS NULL ORDER BY created_at DESC LIMIT ?",
            ))
            .bind(bounded_limit)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(&self.render(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications ORDER BY created_at DESC LIMIT ?",
            ))
            .bind(bounded_limit)
            .fetch_all(self.pool())
            .await?
        };
        Ok(rows.iter().map(mappers::row_to_notification).collect())
    }

    async fn mark_notification_read(
        &self,
        notification_id: Uuid,
    ) -> Result<Option<Notification>, SendableError> {
        let columns = "id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";

        // mysql has no UPDATE ... RETURNING, so update then read the row back by id.
        if self.dialect() == SqlDialect::MySql {
            sqlx::query(
                &self
                    .render("UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ?"),
            )
            .bind(Utc::now().timestamp())
            .bind(notification_id)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(
                &self.render(&format!("SELECT {columns} FROM notifications WHERE id = ?",)),
            )
            .bind(notification_id)
            .fetch_optional(self.pool())
            .await?;
            return Ok(row.map(|row| mappers::row_to_notification(&row)));
        }

        let row = sqlx::query(&self.render(&format!(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ? RETURNING {columns}",
        )))
        .bind(Utc::now().timestamp())
        .bind(notification_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_notification(&row)))
    }

    async fn mark_all_notifications_read(&self) -> Result<u64, SendableError> {
        let result =
            sqlx::query(&self.render("UPDATE notifications SET read_at = ? WHERE read_at IS NULL"))
                .bind(Utc::now().timestamp())
                .execute(self.pool())
                .await?;
        Ok(result.affected())
    }

    async fn upsert_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> Result<(), SendableError> {
        let conflict = queries::on_conflict_update(
            self.dialect(),
            "kind, scope, name",
            &["value", "updated_at"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO settings (kind, scope, name, value, updated_at) VALUES (?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(kind.as_str())
        .bind(scope)
        .bind(name)
        .bind(value)
        .bind(updated_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT kind, scope, name, value, updated_at FROM settings WHERE kind = ? AND scope = ? AND name = ?",
        ))
        .bind(kind.as_str())
        .bind(scope)
        .bind(name)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_setting(&row)))
    }

    async fn delete_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM settings WHERE kind = ? AND scope = ? AND name = ?"))
            .bind(kind.as_str())
            .bind(scope)
            .bind(name)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn list_settings(&self) -> Result<Vec<SettingRecord>, SendableError> {
        let rows = sqlx::query(
            "SELECT kind, scope, name, value, updated_at FROM settings ORDER BY kind, scope, name",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_setting).collect())
    }

    async fn create_user(
        &self,
        username: String,
        email: Option<String>,
        is_admin: bool,
        password_hash: Option<String>,
    ) -> Result<User, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO users (id, username, email, is_admin, disabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&username)
        .bind(&email)
        .bind(is_admin)
        .bind(false)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        if let Some(hash) = password_hash {
            sqlx::query(&self.render(
                "INSERT INTO user_identities (id, user_id, provider, subject, password_hash, created_at) VALUES (?, ?, 'local', ?, ?, ?)",
            ))
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(&username)
            .bind(&hash)
            .bind(now)
            .execute(self.pool())
            .await?;
        }
        let at = DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now);
        Ok(User {
            id: Some(id),
            username,
            email,
            is_admin,
            disabled: false,
            created_at: at,
            updated_at: at,
        })
    }

    async fn fetch_user(&self, id: Uuid) -> Result<Option<User>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, username, email, is_admin, disabled, created_at, updated_at FROM users WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_user(&row)))
    }

    async fn fetch_user_by_username(
        &self,
        username: String,
    ) -> Result<Option<User>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, username, email, is_admin, disabled, created_at, updated_at FROM users WHERE username = ?",
        ))
        .bind(username)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_user(&row)))
    }

    async fn fetch_local_credential(
        &self,
        username: String,
    ) -> Result<Option<LocalCredential>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT u.id, u.username, u.email, u.is_admin, u.disabled, u.created_at, u.updated_at, i.password_hash \
             FROM users u JOIN user_identities i ON i.user_id = u.id \
             WHERE i.provider = 'local' AND i.subject = ? AND i.password_hash IS NOT NULL",
        ))
        .bind(username)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_local_credential(&row)))
    }

    async fn list_users(&self) -> Result<Vec<User>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, username, email, is_admin, disabled, created_at, updated_at FROM users ORDER BY username",
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_user).collect())
    }

    async fn count_users(&self) -> Result<i64, SendableError> {
        let row = sqlx::query(&self.render("SELECT COUNT(*) AS user_count FROM users"))
            .fetch_one(self.pool())
            .await?;
        Ok(row.get::<i64, _>("user_count"))
    }

    async fn update_user(
        &self,
        id: Uuid,
        email: Option<String>,
        is_admin: Option<bool>,
        disabled: Option<bool>,
    ) -> Result<User, SendableError> {
        let Some(current) = self.fetch_user(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("User {id} not found"),
            )));
        };
        let email = email.or(current.email);
        let is_admin = is_admin.unwrap_or(current.is_admin);
        let disabled = disabled.unwrap_or(current.disabled);
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE users SET email = ?, is_admin = ?, disabled = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(&email)
        .bind(is_admin)
        .bind(disabled)
        .bind(now)
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(User {
            id: Some(id),
            username: current.username,
            email,
            is_admin,
            disabled,
            created_at: current.created_at,
            updated_at: DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn set_local_password(
        &self,
        user_id: Uuid,
        password_hash: String,
    ) -> Result<(), SendableError> {
        let Some(user) = self.fetch_user(user_id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("User {user_id} not found"),
            )));
        };
        // replace any existing local identity so the row stays unique on (provider, subject).
        sqlx::query(
            &self.render("DELETE FROM user_identities WHERE user_id = ? AND provider = 'local'"),
        )
        .bind(user_id)
        .execute(self.pool())
        .await?;
        sqlx::query(&self.render(
            "INSERT INTO user_identities (id, user_id, provider, subject, password_hash, created_at) VALUES (?, ?, 'local', ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(&user.username)
        .bind(&password_hash)
        .bind(Utc::now().timestamp())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn delete_user(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM auth_sessions WHERE user_id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        sqlx::query(&self.render("DELETE FROM user_identities WHERE user_id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        sqlx::query(&self.render("DELETE FROM team_members WHERE user_id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        sqlx::query(&self.render(
            "DELETE FROM resource_grants WHERE principal_type = 'user' AND principal_id = ?",
        ))
        .bind(id)
        .execute(self.pool())
        .await?;
        sqlx::query(&self.render("DELETE FROM users WHERE id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn create_api_key(&self, record: ApiKeyRecord) -> Result<ApiKey, SendableError> {
        let id = record.key.id.unwrap_or_else(Uuid::now_v7);
        let created = record.key.created_at.timestamp();
        let last_used = record.key.last_used_at.map(|t| t.timestamp());
        let expires = record.key.expires_at.map(|t| t.timestamp());
        sqlx::query(&self.render(
            "INSERT INTO api_keys (id, name, user_id, is_service, is_admin, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&record.key.name)
        .bind(record.key.user_id)
        .bind(record.key.is_service)
        .bind(record.is_admin)
        .bind(&record.key.key_prefix)
        .bind(&record.key_hash)
        .bind(last_used)
        .bind(expires)
        .bind(record.key.disabled)
        .bind(created)
        .execute(self.pool())
        .await?;
        let mut stored = record.key;
        stored.id = Some(id);
        Ok(stored)
    }

    async fn fetch_api_key_by_prefix(
        &self,
        prefix: String,
    ) -> Result<Option<ApiKeyRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, user_id, is_service, is_admin, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at FROM api_keys WHERE key_prefix = ?",
        ))
        .bind(prefix)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_api_key_record(&row)))
    }

    async fn fetch_api_key(&self, id: Uuid) -> Result<Option<ApiKeyRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, user_id, is_service, is_admin, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at FROM api_keys WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_api_key_record(&row)))
    }

    async fn list_api_keys(&self, user_id: Option<Uuid>) -> Result<Vec<ApiKey>, SendableError> {
        let columns = "id, name, user_id, is_service, key_prefix, last_used_at, expires_at, disabled, created_at";
        let rows = match user_id {
            Some(uid) => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM api_keys WHERE user_id = ? ORDER BY created_at DESC"
                )))
                .bind(uid)
                .fetch_all(self.pool())
                .await?
            }
            None => {
                sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM api_keys ORDER BY created_at DESC"
                )))
                .fetch_all(self.pool())
                .await?
            }
        };
        Ok(rows.iter().map(mappers::row_to_api_key).collect())
    }

    async fn revoke_api_key(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE api_keys SET disabled = ? WHERE id = ?"))
            .bind(true)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn update_api_key(
        &self,
        id: Uuid,
        name: Option<String>,
        expires_at: Option<Option<DateTime<Utc>>>,
        disabled: Option<bool>,
    ) -> Result<ApiKey, SendableError> {
        let Some(record) = self.fetch_api_key(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("API key {id} not found"),
            )));
        };
        let mut key = record.key;
        let next_name = name.unwrap_or_else(|| key.name.clone());
        let next_expires_at = expires_at.unwrap_or(key.expires_at);
        let next_disabled = disabled.unwrap_or(key.disabled);
        sqlx::query(
            &self.render("UPDATE api_keys SET name = ?, expires_at = ?, disabled = ? WHERE id = ?"),
        )
        .bind(&next_name)
        .bind(next_expires_at.map(|t| t.timestamp()))
        .bind(next_disabled)
        .bind(id)
        .execute(self.pool())
        .await?;
        key.name = next_name;
        key.expires_at = next_expires_at;
        key.disabled = next_disabled;
        Ok(key)
    }

    async fn touch_api_key(&self, id: Uuid, last_used_at: i64) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE api_keys SET last_used_at = ? WHERE id = ?"))
            .bind(last_used_at)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn create_session(&self, session: AuthSession) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "INSERT INTO auth_sessions (id, user_id, refresh_token_hash, expires_at, revoked, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(session.id)
        .bind(session.user_id)
        .bind(&session.refresh_token_hash)
        .bind(session.expires_at.timestamp())
        .bind(session.revoked)
        .bind(Utc::now().timestamp())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_session_by_hash(
        &self,
        refresh_token_hash: String,
    ) -> Result<Option<AuthSession>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, user_id, refresh_token_hash, expires_at, revoked FROM auth_sessions WHERE refresh_token_hash = ? AND revoked = ?",
        ))
        .bind(refresh_token_hash)
        .bind(false)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_auth_session(&row)))
    }

    async fn revoke_session(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE auth_sessions SET revoked = ? WHERE id = ?"))
            .bind(true)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn revoke_user_sessions(&self, user_id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE auth_sessions SET revoked = ? WHERE user_id = ?"))
            .bind(true)
            .bind(user_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn create_team(&self, name: String) -> Result<Team, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render("INSERT INTO teams (id, name, created_at) VALUES (?, ?, ?)"))
            .bind(id)
            .bind(&name)
            .bind(now)
            .execute(self.pool())
            .await?;
        Ok(Team {
            id: Some(id),
            name,
            created_at: DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn update_team(&self, id: Uuid, name: String) -> Result<Team, SendableError> {
        let Some(current) = self
            .list_teams()
            .await?
            .into_iter()
            .find(|team| team.id == Some(id))
        else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Team {id} not found"),
            )));
        };
        sqlx::query(&self.render("UPDATE teams SET name = ? WHERE id = ?"))
            .bind(&name)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(Team {
            id: Some(id),
            name,
            created_at: current.created_at,
        })
    }

    async fn list_teams(&self) -> Result<Vec<Team>, SendableError> {
        let rows =
            sqlx::query(&self.render("SELECT id, name, created_at FROM teams ORDER BY name"))
                .fetch_all(self.pool())
                .await?;
        Ok(rows.iter().map(mappers::row_to_team).collect())
    }

    async fn delete_team(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM team_members WHERE team_id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        sqlx::query(&self.render(
            "DELETE FROM resource_grants WHERE principal_type = 'team' AND principal_id = ?",
        ))
        .bind(id)
        .execute(self.pool())
        .await?;
        sqlx::query(&self.render("DELETE FROM teams WHERE id = ?"))
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn add_team_member(&self, team_id: Uuid, user_id: Uuid) -> Result<(), SendableError> {
        // delete-then-insert keeps the (team, user) pair idempotent without a dialect-specific upsert.
        sqlx::query(&self.render("DELETE FROM team_members WHERE team_id = ? AND user_id = ?"))
            .bind(team_id)
            .bind(user_id)
            .execute(self.pool())
            .await?;
        sqlx::query(&self.render("INSERT INTO team_members (team_id, user_id) VALUES (?, ?)"))
            .bind(team_id)
            .bind(user_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM team_members WHERE team_id = ? AND user_id = ?"))
            .bind(team_id)
            .bind(user_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn list_user_team_ids(&self, user_id: Uuid) -> Result<Vec<Uuid>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT team_id FROM team_members WHERE user_id = ?"))
            .bind(user_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|row| row.get::<Uuid, _>("team_id"))
            .collect())
    }

    async fn list_user_teams(&self, user_id: Uuid) -> Result<Vec<Team>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT t.id, t.name, t.created_at \
             FROM teams t \
             INNER JOIN team_members tm ON tm.team_id = t.id \
             WHERE tm.user_id = ? \
             ORDER BY t.name",
        ))
        .bind(user_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_team).collect())
    }

    async fn list_team_members(&self, team_id: Uuid) -> Result<Vec<User>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT u.id, u.username, u.email, u.is_admin, u.disabled, u.created_at, u.updated_at \
             FROM users u \
             INNER JOIN team_members tm ON tm.user_id = u.id \
             WHERE tm.team_id = ? \
             ORDER BY u.username",
        ))
        .bind(team_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_user).collect())
    }

    async fn create_grant(&self, grant: Grant) -> Result<Grant, SendableError> {
        let id = grant.id.unwrap_or_else(Uuid::now_v7);
        let now = Utc::now().timestamp();
        let conflict = queries::on_conflict_update(
            self.dialect(),
            "resource_type, resource_id, principal_type, principal_id",
            &["permission"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO resource_grants (id, resource_type, resource_id, principal_type, principal_id, permission, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(id)
        .bind(grant.resource_type.as_str())
        .bind(grant.resource_id)
        .bind(grant.principal_type.as_str())
        .bind(grant.principal_id)
        .bind(grant.permission.as_str())
        .bind(now)
        .execute(self.pool())
        .await?;
        // read back the canonical row (an upsert keeps the original id).
        let row = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND resource_id = ? AND principal_type = ? AND principal_id = ?",
        ))
        .bind(grant.resource_type.as_str())
        .bind(grant.resource_id)
        .bind(grant.principal_type.as_str())
        .bind(grant.principal_id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_grant(&row))
    }

    async fn revoke_grant(&self, grant_id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM resource_grants WHERE id = ?"))
            .bind(grant_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn list_grants(
        &self,
        resource_type: String,
        resource_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND resource_id = ? ORDER BY created_at",
        ))
        .bind(resource_type)
        .bind(resource_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }

    async fn list_user_grants(
        &self,
        resource_type: String,
        user_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND principal_type = 'user' AND principal_id = ?",
        ))
        .bind(resource_type)
        .bind(user_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }

    async fn list_team_grants(
        &self,
        resource_type: String,
        team_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND principal_type = 'team' AND principal_id = ?",
        ))
        .bind(resource_type)
        .bind(team_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }
}
