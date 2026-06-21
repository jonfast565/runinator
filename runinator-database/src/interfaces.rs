use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_comm::{ActionCommand, ActionDispatchRecord, WorkflowResultEvent};
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
        NewWorkflowRunDeliverable, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunDeliverable, WorkflowStatus, WorkflowTrigger,
    },
};

use crate::archive::{ArchiveMark, ArchiveRow, ArchiveTable};

/// Core persistence operations for Runinator.
pub trait DatabaseImpl: Send + Sync + 'static {
    /// Execute initialization scripts for the database.
    fn run_init_scripts(
        &self,
        paths: &[String],
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Mark old rows that are eligible for archival. Marking is idempotent.
    fn mark_archive_candidates(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Claim archive marks for one archiver process under a short lease.
    fn claim_archive_marks(
        &self,
        archiver_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ArchiveMark>, SendableError>> + Send;

    /// Fetch source rows for claimed archive marks, rechecking eligibility at read time.
    fn fetch_archive_rows(
        &self,
        marks: Vec<ArchiveMark>,
    ) -> impl Future<Output = Result<Vec<ArchiveRow>, SendableError>> + Send;

    /// Delete archived source rows by exact table/id pairs.
    fn delete_archive_rows(
        &self,
        rows: Vec<ArchiveRow>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Mark archive ledger rows as archived after their source rows were deleted.
    fn complete_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Release archive marks after a failed archival attempt.
    fn fail_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
        error: String,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Fetch all runs filtered by their current status.
    fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;

    /// Update the status and output of a specific run.
    fn update_run_status(
        &self,
        run_id: Uuid,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Append a log chunk to an active run.
    fn append_run_chunk(
        &self,
        run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<RunChunk, SendableError>> + Send;

    /// Fetch log chunks for a run with pagination.
    fn fetch_run_chunks(
        &self,
        run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RunChunk>, SendableError>> + Send;

    /// Associate a new artifact with a run.
    fn add_run_artifact(
        &self,
        run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<RunArtifact, SendableError>> + Send;

    /// Fetch all artifacts produced by a specific run.
    fn fetch_run_artifacts(
        &self,
        run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch every artifact across all runs, most-recent first.
    fn fetch_all_artifacts(
        &self,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch a single artifact by its identifier.
    fn fetch_artifact(
        &self,
        artifact_id: Uuid,
    ) -> impl Future<Output = Result<Option<RunArtifact>, SendableError>> + Send;

    /// Create or update a workflow definition.
    fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;

    /// Insert a workflow as a new row, ignoring any id and never updating an existing one.
    /// Used to duplicate a workflow into a sibling version that shares its name.
    fn insert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;

    /// Fetch all workflow definitions.
    fn fetch_workflows(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch a workflow definition by its identifier.
    fn fetch_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch a workflow definition by its unique display name.
    fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Delete a workflow and its associated metadata.
    fn delete_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create or update a workflow trigger.
    fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> impl Future<Output = Result<WorkflowTrigger, SendableError>> + Send;

    /// Fetch all triggers for a workflow definition.
    fn fetch_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Fetch a workflow trigger by identifier.
    fn fetch_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowTrigger>, SendableError>> + Send;

    /// Delete a workflow trigger.
    fn delete_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch enabled triggers that should fire at or before the provided instant.
    fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Update the next execution instant for a workflow trigger.
    fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: Uuid,
        next_execution: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically fire due cron triggers and return the workflow runs created by this claim.
    fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Create a new instance of a workflow.
    fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

    /// Fetch workflow runs filtered by status.
    fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Claim open workflow runs for scheduler processing until the supplied lease instant.
    fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Renew a workflow run claim held by a scheduler.
    fn renew_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Release a workflow run claim held by a scheduler.
    fn release_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch recent workflow runs across all workflow definitions.
    fn fetch_recent_workflow_runs(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch all runs for a specific workflow definition.
    fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch workflow runs by display name, optionally restricted to open runs.
    fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Update the top-level status of a workflow run.
    fn update_workflow_run_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Set or clear the user-facing display name of a workflow run.
    fn set_workflow_run_name(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a workflow run summary by its identifier.
    fn fetch_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowRun>, SendableError>> + Send;

    /// Create a new node execution record within a workflow run.
    fn create_workflow_node_run(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        parameters: Value,
    ) -> impl Future<Output = Result<WorkflowNodeRun, SendableError>> + Send;

    /// Update the status and state of a specific node execution.
    #[allow(clippy::too_many_arguments)]
    fn update_workflow_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all node execution records for a workflow run.
    fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    /// Acquire the executor lease for a node run, returning whether it was acquired. The claim only
    /// succeeds when no live executor holds the slot (unclaimed, or the prior claim predates
    /// `stale_before`), making duplicate/redelivered executions of the same node run mutually
    /// exclusive.
    fn claim_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Clear the current executor and record the last executor for a node run.
    fn release_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        released_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a node execution record by its identifier.
    fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowNodeRun>, SendableError>> + Send;

    /// Fetch all node execution records in a given status across every run. Used to route an
    /// inbound signal to a parked node by correlation key without knowing its run id.
    fn fetch_workflow_node_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    /// Append a log chunk to a workflow node run.
    fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<WorkflowNodeRunChunk, SendableError>> + Send;

    /// Fetch log chunks for a workflow node run with pagination.
    fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunChunk>, SendableError>> + Send;

    /// Associate an artifact with a workflow node run.
    fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<WorkflowNodeRunArtifact, SendableError>> + Send;

    /// Fetch artifacts for a workflow node run.
    fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunArtifact>, SendableError>> + Send;

    /// Fetch every node artifact produced across a whole workflow run.
    fn fetch_workflow_node_run_artifacts_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunArtifact>, SendableError>> + Send;

    /// Promote a node artifact to a run-level deliverable.
    fn add_workflow_run_deliverable(
        &self,
        deliverable: &NewWorkflowRunDeliverable,
    ) -> impl Future<Output = Result<WorkflowRunDeliverable, SendableError>> + Send;

    /// Fetch run-level deliverables for a workflow run.
    fn fetch_workflow_run_deliverables(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowRunDeliverable>, SendableError>> + Send;

    /// Apply a workflow result event once; returns false for duplicate events.
    fn apply_workflow_result_event(
        &self,
        event: &WorkflowResultEvent,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Append an internal orchestration event once; returns false for duplicate event ids.
    fn append_orchestration_event(
        &self,
        event: &NewOrchestrationEvent,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch internal orchestration events for a workflow run.
    fn fetch_orchestration_events(
        &self,
        workflow_run_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationEvent>, SendableError>> + Send;

    /// Enqueue a state-machine node for scheduler processing.
    fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Claim ready nodes for scheduler processing until the supplied lease instant.
    fn claim_ready_nodes(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Fetch a ready-node row by identifier.
    fn fetch_ready_node(
        &self,
        ready_node_id: Uuid,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Mark a claimed ready-node row complete.
    fn complete_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch ready-node rows still pending drive (uncompleted and not currently claimed), so the
    /// web service can announce them on the wake channel. Includes future `ready_at` rows.
    fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Claim a single ready-node row by id for drive, leasing it to `scheduler_id`.
    fn claim_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Release a claimed ready-node row back to the queued state so it can be re-driven.
    fn release_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Create or update a generic catalog item.
    fn upsert_catalog_item(
        &self,
        item: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch catalog items, optionally filtered by type.
    fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single catalog item by its URI.
    fn fetch_catalog_item(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Register or refresh a runtime replica.
    fn register_replica(
        &self,
        request: ReplicaRegistrationRequest,
        observed_ip: Option<String>,
    ) -> impl Future<Output = Result<ReplicaRecord, SendableError>> + Send;

    /// Refresh a replica heartbeat if the runtime id still matches.
    fn heartbeat_replica(
        &self,
        replica_id: Uuid,
        request: ReplicaHeartbeatRequest,
        observed_ip: Option<String>,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Mark a replica offline if the runtime id still matches.
    fn mark_replica_offline(
        &self,
        replica_id: Uuid,
        runtime_id: String,
    ) -> impl Future<Output = Result<Option<ReplicaRecord>, SendableError>> + Send;

    /// Mark replicas offline that have not sent a heartbeat since the cutoff. returns the count
    /// reaped so callers can log activity.
    fn reap_inactive_replicas(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Hard-delete replicas whose last heartbeat predates the cutoff, clearing historical attribution
    /// pointers first so restrict-mode foreign keys do not block the delete. returns the count purged.
    fn delete_expired_replicas(
        &self,
        cutoff: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Fetch replicas filtered by type and status, deriving stale state from heartbeat age.
    fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<ReplicaRecord>, SendableError>> + Send;

    /// Upsert a provider registration for a worker replica.
    fn upsert_replica_provider_registration(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> impl Future<Output = Result<ReplicaProviderRegistration, SendableError>> + Send;

    /// Fetch provider registrations for a replica.
    fn fetch_replica_provider_registrations(
        &self,
        replica_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ReplicaProviderRegistration>, SendableError>> + Send;

    /// Create a new record in a generic orchestration table.
    fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Update an existing orchestration record.
    fn update_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch orchestration records with optional filters.
    fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<Uuid>,
        external_item_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single orchestration record by its identifier.
    fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Create a gate row (a per-run, per-node automated/policy block).
    fn create_gate(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Update an existing gate row (status/reason/resolution).
    fn update_gate(
        &self,
        gate_id: Uuid,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch gate rows with optional run and status filters.
    fn fetch_gates(
        &self,
        workflow_run_id: Option<Uuid>,
        status: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single gate row by its identifier.
    fn fetch_gate(
        &self,
        gate_id: Uuid,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Persist a dead-lettered broker message for later inspection/replay.
    fn record_dead_letter(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch dead-letter rows, newest first, with an optional channel filter.
    fn fetch_dead_letters(
        &self,
        channel: Option<String>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Append an audit-log entry (auth/authz/sensitive op).
    fn record_audit_log(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch audit-log rows, newest first, with optional actor and action filters.
    fn fetch_audit_log(
        &self,
        actor_id: Option<Uuid>,
        action: Option<String>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Store a result for an idempotency key.
    fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch the result for an idempotency key if it exists.
    fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Store an action dispatch intent for durable scheduler recovery.
    fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> impl Future<Output = Result<ActionDispatchRecord, SendableError>> + Send;

    /// Fetch unpublished action dispatch intents.
    fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ActionDispatchRecord>, SendableError>> + Send;

    /// Claim unpublished action dispatch intents for one publisher.
    fn claim_pending_action_dispatches(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ActionDispatchRecord>, SendableError>> + Send;

    /// Mark an action dispatch as successfully published.
    fn mark_action_dispatch_published(
        &self,
        dispatch_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Record a failed action dispatch publish attempt.
    fn mark_action_dispatch_failed(
        &self,
        dispatch_id: Uuid,
        error: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Persist a notification record.
    fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> impl Future<Output = Result<Notification, SendableError>> + Send;

    /// Fetch notifications, optionally only unread, most-recent first.
    fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Notification>, SendableError>> + Send;

    /// Mark a notification as read; returns the updated row.
    fn mark_notification_read(
        &self,
        notification_id: Uuid,
    ) -> impl Future<Output = Result<Option<Notification>, SendableError>> + Send;

    /// Mark all unread notifications as read; returns the number updated.
    fn mark_all_notifications_read(
        &self,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Insert or replace a setting's stored value (encrypted at rest) and modification time.
    fn upsert_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a single setting's persisted record, or None when it does not exist.
    fn fetch_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<Option<SettingRecord>, SendableError>> + Send;

    /// Delete a setting; succeeds even when the entry is absent.
    fn delete_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// List every stored setting (encrypted values included), ordered by kind/scope/name.
    fn list_settings(
        &self,
    ) -> impl Future<Output = Result<Vec<SettingRecord>, SendableError>> + Send;

    // ---- auth: users, identities, api keys, sessions ----

    /// Create a user and, when `password_hash` is set, a matching local identity.
    fn create_user(
        &self,
        username: String,
        email: Option<String>,
        is_admin: bool,
        password_hash: Option<String>,
    ) -> impl Future<Output = Result<User, SendableError>> + Send;

    /// Fetch a user by id.
    fn fetch_user(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<User>, SendableError>> + Send;

    /// Fetch a user by username.
    fn fetch_user_by_username(
        &self,
        username: String,
    ) -> impl Future<Output = Result<Option<User>, SendableError>> + Send;

    /// Resolve a local login: the user plus the stored argon2 hash for `username`.
    fn fetch_local_credential(
        &self,
        username: String,
    ) -> impl Future<Output = Result<Option<LocalCredential>, SendableError>> + Send;

    /// List all users.
    fn list_users(&self) -> impl Future<Output = Result<Vec<User>, SendableError>> + Send;

    /// Count users (used to decide whether to seed a bootstrap admin).
    fn count_users(&self) -> impl Future<Output = Result<i64, SendableError>> + Send;

    /// Patch a user's mutable fields (None leaves a field unchanged).
    fn update_user(
        &self,
        id: Uuid,
        email: Option<String>,
        is_admin: Option<bool>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<User, SendableError>> + Send;

    /// Set (upsert) a user's local password hash.
    fn set_local_password(
        &self,
        user_id: Uuid,
        password_hash: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a user and their identities/sessions.
    fn delete_user(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create an api key from a fully-formed record (caller supplies the hash).
    fn create_api_key(
        &self,
        record: ApiKeyRecord,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Fetch an api key (incl. hash) by id for administration.
    fn fetch_api_key(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// Fetch an api key (incl. hash) by its public prefix for verification.
    fn fetch_api_key_by_prefix(
        &self,
        prefix: String,
    ) -> impl Future<Output = Result<Option<ApiKeyRecord>, SendableError>> + Send;

    /// List api keys, optionally scoped to one owner.
    fn list_api_keys(
        &self,
        user_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<ApiKey>, SendableError>> + Send;

    /// Disable (revoke) an api key.
    fn revoke_api_key(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Update api key metadata.
    fn update_api_key(
        &self,
        id: Uuid,
        name: Option<String>,
        expires_at: Option<Option<DateTime<Utc>>>,
        disabled: Option<bool>,
    ) -> impl Future<Output = Result<ApiKey, SendableError>> + Send;

    /// Record an api key's last-used timestamp (best effort).
    fn touch_api_key(
        &self,
        id: Uuid,
        last_used_at: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create a refresh session.
    fn create_session(
        &self,
        session: AuthSession,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a non-revoked session by its refresh-token hash.
    fn fetch_session_by_hash(
        &self,
        refresh_token_hash: String,
    ) -> impl Future<Output = Result<Option<AuthSession>, SendableError>> + Send;

    /// Revoke a single session.
    fn revoke_session(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Revoke every session for a user (logout-all / password change).
    fn revoke_user_sessions(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    // ---- authz: teams + resource grants ----

    /// Create a team.
    fn create_team(&self, name: String)
    -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// Rename a team.
    fn update_team(
        &self,
        id: Uuid,
        name: String,
    ) -> impl Future<Output = Result<Team, SendableError>> + Send;

    /// List all teams.
    fn list_teams(&self) -> impl Future<Output = Result<Vec<Team>, SendableError>> + Send;

    /// Delete a team and its memberships.
    fn delete_team(&self, id: Uuid) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Add a user to a team (idempotent).
    fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Remove a user from a team.
    fn remove_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// The team ids a user belongs to (used to resolve effective permissions).
    fn list_user_team_ids(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;

    /// The teams a user belongs to.
    fn list_user_teams(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Team>, SendableError>> + Send;

    /// The users assigned to a team.
    fn list_team_members(
        &self,
        team_id: Uuid,
    ) -> impl Future<Output = Result<Vec<User>, SendableError>> + Send;

    /// Create or update (by resource+principal) a grant.
    fn create_grant(
        &self,
        grant: Grant,
    ) -> impl Future<Output = Result<Grant, SendableError>> + Send;

    /// Revoke a grant by id.
    fn revoke_grant(
        &self,
        grant_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// All grants on a resource.
    fn list_grants(
        &self,
        resource_type: String,
        resource_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;

    /// A user's direct grants of a resource type (for visibility scoping).
    fn list_user_grants(
        &self,
        resource_type: String,
        user_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;

    /// A team's grants of a resource type (for visibility scoping).
    fn list_team_grants(
        &self,
        resource_type: String,
        team_id: Uuid,
    ) -> impl Future<Output = Result<Vec<Grant>, SendableError>> + Send;
}
