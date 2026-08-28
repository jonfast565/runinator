use chrono::{DateTime, Utc};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord};
use runinator_models::value::Value;
use runinator_models::{
    bundles::{PackImportResult, ProviderBundle},
    console::{ConsoleCell, ConsoleSession, ConsoleSessionDetail},
    files::StoredFile,
    functions::{
        FunctionAlias, FunctionArtifact, FunctionCatalogEntry, FunctionInvocationTarget,
        FunctionPackage, FunctionPackageDetail, FunctionVersion,
    },
    notifications::{Notification, NotificationDelivery, NotificationPolicy},
    pipelines::{Pipeline, PipelineMemberAttempt, PipelineRun, PipelineRunDetail, PipelineTrigger},
    providers::ProviderMetadata,
    provisioning::{NodeBackendsResponse, ProvisionedGroup},
    replicas::{ReplicaListResponse, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus},
    revisions::{PipelineRevision, WorkflowRevision},
    schedules::{BackfillResponse, FreezeWindow},
    settings::SettingKind,
    telemetry::ReplicaSampleSeries,
    web::TaskResponse,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectOutputEvent, WorkflowJournalRecord,
        WorkflowVmCursor,
    },
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
            expected: None,
            actual: None,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigResponseSchema {
    pub enabled: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginRequestSchema {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserSchema {
    pub id: Option<Uuid>,
    pub username: String,
    pub email: Option<String>,
    pub disabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponseSchema {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: UserSchema,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RefreshRequestSchema {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskResponseSchema {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)] // variants mirror the stable untagged HTTP response contract.
pub enum ApiResponse {
    TaskResponse(TaskResponse),
    ApiError(ApiError),
    Workflow(WorkflowDefinition),
    WorkflowBundle(WorkflowBundle),
    WorkflowList(Vec<WorkflowDefinition>),
    WorkflowRevision(WorkflowRevision),
    WorkflowRevisionList(Vec<WorkflowRevision>),
    WorkflowTrigger(WorkflowTrigger),
    WorkflowTriggerList(Vec<WorkflowTrigger>),
    Pipeline(Pipeline),
    PipelineList(Vec<Pipeline>),
    PipelineRevision(PipelineRevision),
    PipelineRevisionList(Vec<PipelineRevision>),
    PipelineTrigger(PipelineTrigger),
    PipelineTriggerList(Vec<PipelineTrigger>),
    PipelineRun(PipelineRun),
    PipelineRunDetail(PipelineRunDetail),
    PipelineRunList(Vec<PipelineRun>),
    PipelineMemberAttempt(PipelineMemberAttempt),
    Ingress(IngressResponse),
    IngressAdmission(runinator_models::orchestration::IngressAdmission),
    IngressTimeline(Vec<runinator_models::orchestration::IngressInboxEntry>),
    OrchestrationBinding(runinator_models::orchestration::OrchestrationBinding),
    OrchestrationBindingList(Vec<runinator_models::orchestration::OrchestrationBinding>),
    OrchestrationCorrelationAlias(runinator_models::orchestration::OrchestrationCorrelationAlias),
    OrchestrationCorrelationAliasList(
        Vec<runinator_models::orchestration::OrchestrationCorrelationAlias>,
    ),
    OrchestrationEpochList(Vec<runinator_models::orchestration::OrchestrationEpoch>),
    OrchestrationReductionList(Vec<runinator_models::orchestration::OrchestrationEventReduction>),
    OrchestrationEvidenceList(Vec<runinator_models::orchestration::OrchestrationEvidence>),
    OrchestrationCommandList(Vec<runinator_models::orchestration::OrchestrationCommand>),
    WorkspaceList(Vec<runinator_models::workspaces::WorkspaceLease>),
    OrchestrationAdapter(runinator_models::orchestration::AdapterDefinition),
    OrchestrationAdapterList(Vec<runinator_models::orchestration::AdapterDefinition>),
    OrchestrationAdapterRevision(runinator_models::orchestration::AdapterRevision),
    OrchestrationAdapterRevisionList(Vec<runinator_models::orchestration::AdapterRevision>),
    AdapterKindList(Vec<runinator_models::orchestration::AdapterKindCatalogEntry>),
    ExternalOperationList(Vec<runinator_models::orchestration::ExternalOperation>),
    ExternalOperation(runinator_models::orchestration::ExternalOperation),
    WorkflowRun(WorkflowRunResponse),
    WorkflowRunList(Vec<WorkflowRun>),
    WorkflowNodeRun(WorkflowNodeRun),
    WorkflowNodeRunChunks(Vec<WorkflowNodeRunChunk>),
    WorkflowNodeRunArtifacts(Vec<WorkflowNodeRunArtifact>),
    WorkflowRunArtifacts(Vec<WorkflowRunArtifact>),
    WorkflowContinuation(WorkflowContinuation),
    WorkflowContinuationList(Vec<WorkflowContinuation>),
    WorkflowEffect(WorkflowEffect),
    WorkflowEffectList(Vec<WorkflowEffect>),
    WorkflowEffectOutput(Vec<WorkflowEffectOutputEvent>),
    WorkflowJournal(Vec<WorkflowJournalRecord>),
    WorkflowVmCursors(Vec<WorkflowVmCursor>),
    WorkflowFile(StoredFile),
    WorkflowFileList(Vec<StoredFile>),
    NodeTransitions(Vec<runinator_models::orchestration::NodeTransition>),
    NodeTransitionStats(Vec<runinator_models::orchestration::NodeTransitionStat>),
    Provider(ProviderMetadata),
    ProviderList(Vec<ProviderMetadata>),
    ProviderBundle(ProviderBundle),
    Replica(ReplicaRecord),
    ReplicaList(ReplicaListResponse),
    ReplicaSamples(ReplicaSampleSeries),
    ReplicaProviderRegistration(ReplicaProviderRegistration),
    ReplicaProviderRegistrationList(Vec<ReplicaProviderRegistration>),
    AgentDirective(AgentDirectiveRecord),
    AgentDirectiveList(Vec<AgentDirectiveRecord>),
    NodeBackends(NodeBackendsResponse),
    NodeGroup(ProvisionedGroup),
    NodeGroupList(Vec<ProvisionedGroup>),
    PackImport(PackImportResult),
    JsonValue(Value),
    JsonList(Vec<Value>),
    Notification(Notification),
    NotificationList(Vec<Notification>),
    NotificationPolicy(NotificationPolicy),
    NotificationPolicyList(Vec<NotificationPolicy>),
    NotificationDeliveryList(Vec<NotificationDelivery>),
    FreezeWindow(FreezeWindow),
    FreezeWindowList(Vec<FreezeWindow>),
    Backfill(BackfillResponse),
    FunctionPackageList(Vec<FunctionPackage>),
    FunctionPackage(FunctionPackageDetail),
    FunctionVersion(FunctionVersion),
    FunctionAlias(FunctionAlias),
    FunctionArtifact(FunctionArtifact),
    FunctionCatalog(Vec<FunctionCatalogEntry>),
    // boxed: it nests a whole export, and an unboxed variant would widen every ApiResponse.
    FunctionInvocationTarget(Box<FunctionInvocationTarget>),
    ConsoleSession(ConsoleSession),
    ConsoleSessionList(Vec<ConsoleSession>),
    // boxed: it nests every cell and binding, and an unboxed variant would widen every ApiResponse.
    ConsoleSessionDetail(Box<ConsoleSessionDetail>),
    ConsoleCell(ConsoleCell),
}

#[derive(Debug, Deserialize)]
pub struct CreateAgentDirectiveRequest {
    pub kind: AgentDirectiveKind,
    /// relative deadline for delivery and execution; defaults to five minutes.
    #[serde(default)]
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AgentDirectiveQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub name: Option<String>,
    /// File ids referenced by typed input parameters. Staged inputs are claimed for this run and
    /// library revisions are validated immediately before the VM is nudged.
    #[serde(default)]
    pub file_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowTriggerRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct PipelineRunRequest {
    #[serde(default)]
    pub parameters: Value,
    /// Run an immutable historical pipeline definition instead of the current head.
    #[serde(default)]
    pub revision: Option<i64>,
    /// Start with this member as the sole frontier instead of the graph's entry members.
    #[serde(default)]
    pub start_member: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrchestrationIntentRequest {
    pub intent: String,
    #[serde(default)]
    pub payload: Value,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
pub struct OrchestrationRequeueRequest {
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
pub struct AdapterApplyRequest {
    pub name: String,
    pub kind: String,
    pub kind_version: String,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub secret_bindings: BTreeMap<String, Uuid>,
    #[serde(default)]
    pub identity_configuration: Value,
    #[serde(default)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AdapterEnableRequest {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AdapterTestRequest {
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body_base64: String,
    #[serde(default)]
    pub configuration: Option<Value>,
    #[serde(default)]
    pub secret_bindings: Option<BTreeMap<String, Uuid>>,
}

#[derive(Debug, Deserialize)]
pub struct ExternalOperationResolutionRequest {
    pub resolution: String,
    pub reason: String,
    #[serde(default)]
    pub receipt: Value,
}

/// Opaque provider-neutral event submitted to a workflow or pipeline ingress policy.
#[derive(Debug, Deserialize)]
pub struct IngressEventRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
    #[serde(default)]
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct IngressAdmissionQuery {
    pub scope: String,
    pub correlation_key: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IngressResponse {
    pub admission_id: Uuid,
    pub generation: i64,
    pub disposition: String,
    pub duplicate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_binding_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ManagedRunOverrideRequest {
    /// Required when a platform administrator deliberately bypasses orchestration ownership.
    #[serde(default)]
    pub reason: Option<String>,
    /// Client-generated key used to prevent a retried override request from applying twice.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PipelineMemberRetryRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SettingMoveRequest {
    pub kind: SettingKind,
    pub scope: String,
    pub name: String,
}

/// resolve a pipeline run's pending inquiry (a member with the `Inquire` failure mode paused it).
/// mirrors [`ApprovalResolutionRequest`]'s shape; `decision` plays the approve/reject role.
#[derive(Debug, Deserialize)]
pub struct PipelineRunResolutionRequest {
    pub decision: PipelineRunInquiryDecision,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunInquiryDecision {
    Continue,
    Abort,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusQuery {
    pub status: Option<WorkflowStatus>,
    pub workflow_id: Option<Uuid>,
    pub name: Option<String>,
    pub open: Option<bool>,
    /// caps the unfiltered recent-runs list; clamped server-side. absent uses the default cap.
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub active_node_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub statuses: Vec<WorkflowStatus>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimRenewRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SchedulerRunClaimReleaseRequest {
    pub scheduler_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct WorkflowRunRenameRequest {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SignalDeliveryRequest {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

/// an interrupt asked for from outside the run. `source` defaults to `external`, which is the one
/// a caller normally has any business raising; the field exists so an operator can also drive the
/// other sources by hand. `continuation_id` names one thread of control in a fanned-out run, and
/// is omitted to let whichever real thread drives next take it.
#[derive(Debug, Deserialize)]
pub struct InterruptRequest {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub payload: Value,
    /// The thread to interrupt.
    #[serde(default)]
    pub continuation_id: Option<Uuid>,
}

/// an event delivered to a parked `event_source` node. `type` selects which subscriptions match;
/// the rest of the body is the payload the node's filter and body see.
#[derive(Debug, Deserialize)]
pub struct EventDeliveryRequest {
    #[serde(rename = "type", default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub data: Value,
}

/// inbound webhook that routes a signal to a parked node by business correlation key (e.g. a ticket
/// key or PR number) rather than a run id, so external systems need not track run ids.
#[derive(Debug, Deserialize)]
pub struct WebhookSignalRequest {
    pub name: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct WorkflowRunReplayRequest {
    #[serde(default)]
    pub from_step_id: Option<String>,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
    pub execution_state: runinator_models::workflow_state::WorkflowExecutionState,
}

impl WorkflowRunResponse {
    pub fn new(run: WorkflowRun, nodes: Vec<WorkflowNodeRun>) -> Self {
        let execution_state = run.execution_state.clone();
        Self {
            run,
            nodes,
            execution_state,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    pub item_type: Option<String>,
    pub uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AutomationRecordQuery {
    pub workflow_run_id: Option<Uuid>,
    pub external_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalResolutionRequest {
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub output_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct GateQuery {
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeadLetterQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GateResolutionRequest {
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IdempotencyRequest {
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
}

#[derive(Debug, Deserialize)]
pub struct CredentialQuery {
    pub scope: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub kind: SettingKind,
}

#[derive(Debug, Deserialize)]
pub struct ReplicaQuery {
    pub replica_type: Option<runinator_models::replicas::ReplicaKind>,
    pub status: Option<ReplicaStatus>,
}

#[derive(Debug, Deserialize)]
pub struct ReplicaSampleQuery {
    /// look-back window in seconds; defaults to the last hour when absent.
    pub since_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CredentialPutRequest {
    pub scope: String,
    pub name: String,
    pub value: Value,
    // declared json-schema, required once per config slot; ignored for secrets.
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub kind: SettingKind,
    /// optional RFC 3339 expiry for secrets; rejected for config values.
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}
