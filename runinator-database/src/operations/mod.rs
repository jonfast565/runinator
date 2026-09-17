//! every `DatabaseImpl` method, written once over any `SqlBackend`.
//!
//! the bodies are authored in sqlite-style `?` placeholders and rendered per dialect; the handful of
//! genuinely divergent fragments (boolean literal, row locking, insert-or-ignore form, and the
//! postgres no-id insert path) are the only places that branch on `self.dialect()`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use runinator_comm::{
    AgentDirectiveKind, AgentDirectiveRecord, AgentDirectiveResult, AgentDirectiveStatus,
};
use runinator_models::value::{Map, Value};
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::workflow_vm::{
    WORKFLOW_JOURNAL_VERSION, WorkflowContinuation, WorkflowJournalEntry, WorkflowModule,
};
use runinator_models::{
    ai_usage::{AiCostSource, AiTokenUsage, AiUsageRecord},
    auth::{
        AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, AuthContext,
        AuthSession, Grant, LocalCredential, Team, User,
    },
    billing::{OrgQuota, OrgResourceGroup, UsageSample},
    console::{
        ConsoleBinding, ConsoleCell, ConsoleCellKind, ConsoleCellStatus, ConsoleFunction,
        ConsoleSession, NewConsoleCell, NewConsoleFunction,
    },
    errors::SendableError,
    execution_profiles::{ExecutionProfile, ExecutionProfileRevision},
    files::{FileScope, StoredFile},
    functions::{
        FunctionAdapterWorkflow, FunctionAlias, FunctionArtifact, FunctionCatalogEntry,
        FunctionExport, FunctionPackage, FunctionVersion, NewFunctionVersion,
    },
    notifications::{
        ConversationReceipt, NewNotification, NewNotificationPolicy, Notification,
        NotificationDelivery, NotificationDeliveryStatus, NotificationEvent,
        NotificationInteraction, NotificationPolicy,
    },
    orchestration::IdempotencyClaim,
    orgs::{OrgMembership, OrgRole, Organization},
    pipelines::{
        Pipeline, PipelineExecutionContext, PipelineMemberAttempt, PipelineMemberAttemptStatus,
        PipelineRun, PipelineTrigger,
    },
    rbac::{
        ResourceOwnership, Role, RoleAssignment, ScopeKind, ScopeRef, ServiceAccount, TeamRole,
    },
    replicas::{
        ReplicaHeartbeatRequest, ReplicaKind, ReplicaProviderRegistration,
        ReplicaProviderRegistrationRequest, ReplicaRecord, ReplicaRegistrationRequest,
        ReplicaStatus, WorkflowRunProvenance,
    },
    revisions::{PipelineRevision, WorkflowRevision},
    schedules::{
        BackfillRequest, BackfillResponse, CalendarSubscription, CatchupPolicy, ConcurrencyPolicy,
        DEFAULT_BACKFILL_LIMIT, FiringOutcome, FreezeWindow, MAX_BACKFILL_LIMIT,
        NewCalendarSubscriptionRecord, NewFreezeWindow, TriggerCatchup, TriggerFiringBatch,
        WorkflowConcurrency,
    },
    settings::{SettingKind, SettingRecord},
    telemetry::ReplicaSample,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus, WorkflowTrigger},
    workspaces::{NewWorkspaceLease, WorkspaceLease, WorkspaceStatus},
};
use sqlx::{ColumnIndex, Database, Decode, Encode, Executor, IntoArguments, Row, Type};
use uuid::Uuid;

use crate::{
    backend::{RowsAffected, SqlBackend, SqlStore, retry_delete},
    common::{
        PipelineTriggerExt, WorkflowTriggerExt, json_metadata, json_opt_i64, json_opt_str,
        json_opt_uuid, json_str, schedule_from_configuration, schedule_slots_between, status_list,
    },
    mappers,
    queries::SqlDialect,
};
use runinator_store::{
    archive::{ArchiveMark, ArchiveRow, ArchiveTable},
    prelude::*,
};

const WORKFLOW_RUN_COLUMNS: &str = "id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state_version, created_at, started_at, finished_at, message, name, correlation_key, pipeline_run_id, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_request_host, trigger_request_ip, trigger_metadata";
const WORKFLOW_COLUMNS: &str = "id, name, resource_key, namespace, org_id, version, enabled, input_schema, output_schema, definition, created_at, updated_at";
/// every column `mappers::row_to_ready_node` reads. hoisted because this list appeared verbatim in
/// seven places, and a mapper reading a column one of them forgot to select panics only on that one
/// code path.
const REPLICA_COLUMNS: &str = "replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at, kicked_at, registered_by_principal_id, registered_by_kind, registered_by_org_id";
const REPLICA_PROVIDER_COLUMNS: &str = "replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at";
const AGENT_DIRECTIVE_COLUMNS: &str = "directive_id, replica_id, kind_json, state, issued_at, expires_at, published_at, completed_at, payload_json, message, attempts, claimed_at, claimed_by_runtime_id";
const PIPELINE_COLUMNS: &str = "id, name, resource_key, namespace, description, org_id, enabled, defaults, metadata, graph, concurrency, created_at, updated_at";
const PIPELINE_REVISION_COLUMNS: &str = "id, pipeline_id, revision, digest, name, description, graph, concurrency, defaults, metadata, source, actor_id, actor_kind, note, created_at";
const PIPELINE_TRIGGER_COLUMNS: &str = "id, pipeline_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at";
const PIPELINE_RUN_COLUMNS: &str = "id, pipeline_id, pipeline_snapshot, status, parameters, state, created_at, started_at, finished_at, message, trigger_source_kind, trigger_actor_type, trigger_actor_replica_id, trigger_actor_display_name, trigger_metadata, orchestration_binding_id, execution_epoch, start_member";
const PIPELINE_MEMBER_ATTEMPT_COLUMNS: &str = "id, pipeline_run_id, member_key, workflow_id, attempt, workflow_run_id, status, parameters, result, message, created_at, started_at, finished_at";

const NOTIFICATION_POLICY_COLUMNS: &str = "id, org_id, workflow_id, name, event, severity, channel, provider, provider_function, interactive, target, threshold_seconds, enabled, managed_by, configuration, created_at, updated_at";
const NOTIFICATION_COLUMNS: &str = "id, org_id, source_resource_type, source_resource_id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at";
const NOTIFICATION_DELIVERY_COLUMNS: &str = "id, notification_id, policy_id, channel, provider, provider_function, target, workflow_run_id, status, attempts, last_error, response_json, command_json, published_at, claimed_by, claimed_until, created_at, updated_at";
const NOTIFICATION_INTERACTION_COLUMNS: &str = "id, notification_id, org_id, target_json, actions_json, state, resolved_action, resolved_by, resolved_at, created_at, updated_at";

/// true when an insert lost a unique-constraint race rather than failing for a reason worth
/// surfacing. lets a caller that assigns its own sequence number recompute and retry.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// shared insert for the create and pack-reconcile paths, which differ only in how the id is chosen.

const FREEZE_WINDOW_COLUMNS: &str = "id, org_id, workflow_id, name, reason, starts_at, ends_at, schedule, enabled, created_at, updated_at";

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

/// a correlated predicate keeping a `workflow_triggers` row out of the due set while its workflow is
/// disabled. a trigger has an `enabled` flag of its own, but disabling the *workflow* is the switch
/// operators reach for, and it has to stop the schedule too. enforced in sql rather than skipped in
/// the firing loop for the same reason a freeze window is: a disabled workflow's slot stays due, so
/// it would otherwise sit at the head of the due ordering and crowd live triggers out of the claim
/// limit. re-enabling leaves the stale slot due, which the trigger's catch-up policy then decides
/// about — the same handling a trigger gets when it comes out of a freeze.
fn workflow_enabled_sql(dialect: SqlDialect) -> String {
    format!(
        "SELECT 1 FROM workflows w WHERE w.id = workflow_triggers.workflow_id AND w.enabled = {}",
        dialect.bool_true(),
    )
}

/// the per-slot steps of a cron firing, shared by the trigger loop and the manual backfill so both
/// paths record firings, snapshot workflows, and start runs the same way.

/// sql/row-mapping for one archive table. a local trait since `ArchiveTable` lives in
/// `runinator-store`, which stays free of sql text.
#[derive(Clone, Copy)]
pub(crate) enum ArchiveColumnKind {
    Uuid,
    OptionalUuid,
    Text,
    OptionalText,
    Integer,
    OptionalInteger,
    Boolean,
}

macro_rules! archive_columns {
    ($($name:literal => $kind:ident),+ $(,)?) => {
        &[$(ArchiveColumn { name: $name, kind: ArchiveColumnKind::$kind }),+]
    };
}

#[cfg(all(test, feature = "sqlite"))]
pub(crate) fn archived_column_names(table: ArchiveTable) -> Vec<&'static str> {
    table
        .archive_columns()
        .iter()
        .map(|column| column.name)
        .collect()
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
        eligible_before: timestamp_to_utc(row.get("eligible_before"))?,
        archive_day: row.get("archive_day"),
    })
}

// `DatabaseImpl` is foreign (it lives in the sqlx-free `runinator-store`), so the orphan rule
// forbids implementing it on a bare `B`. `SqlStore<B>` is local and forwards `SqlBackend`, which
// keeps every body below generic over the driver exactly as before.
// the subset the workflow state machine calls. split out so `runinator-runtime` can bound on
// `RuntimeStore` instead of the whole store; the bodies are unchanged and still generic over the
// driver. the where clause below is repeated verbatim from the `DatabaseImpl` impl: both need the
// same sqlx encode/decode bounds, and spelling them out beats hiding them in a macro.

// the generic implementation, one file per role trait.
mod ai_usage;
mod archive;
mod auth;
mod automation;
mod console;
mod database_impl;
mod definitions;
mod delivery;
mod execution_profiles;
mod execution_state_sql;
mod files;
mod functions;
mod ingress;
mod notifications;
mod orchestrations;
mod orgs;
mod pack_transaction;
mod rbac;
mod replicas;
mod runs;
mod runtime;
mod schedules;
mod settings;
mod workflow_vm;
mod workspaces;

mod durable_workspaces;
mod workspace_retention;

mod adapter_control;

mod workspace_transfers;

mod notification_sql_ext;
use notification_sql_ext::NotificationSqlExt;

mod trigger_run_context;
use trigger_run_context::TriggerRunContext;

mod schedule_sql_ext;
use schedule_sql_ext::ScheduleSqlExt;

mod archive_sql_ext;
use archive_sql_ext::ArchiveSqlExt;

mod archive_column;
pub(crate) use archive_column::ArchiveColumn;

mod archive_table_sql;
pub(crate) use archive_table_sql::ArchiveTableSql;
