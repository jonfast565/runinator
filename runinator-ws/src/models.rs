use chrono::{DateTime, Utc};
use runinator_models::value::Value;
use runinator_models::{
    bundles::{PackImportResult, ProviderBundle, SecretBundle},
    notifications::Notification,
    pipelines::Pipeline,
    providers::ProviderMetadata,
    provisioning::{NodeBackendsResponse, ProvisionedGroup},
    replicas::{ReplicaListResponse, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus},
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    settings::SettingKind,
    telemetry::ReplicaSampleSeries,
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
    },
};
use serde::{Deserialize, Serialize};
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
    pub is_admin: bool,
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
pub enum ApiResponse {
    TaskResponse(TaskResponse),
    ApiError(ApiError),
    RunList(Vec<RunSummary>),
    RunChunks(Vec<RunChunk>),
    RunArtifacts(Vec<RunArtifact>),
    Workflow(WorkflowDefinition),
    WorkflowBundle(WorkflowBundle),
    WorkflowList(Vec<WorkflowDefinition>),
    WorkflowTrigger(WorkflowTrigger),
    WorkflowTriggerList(Vec<WorkflowTrigger>),
    Pipeline(Pipeline),
    PipelineList(Vec<Pipeline>),
    WorkflowRun(WorkflowRunResponse),
    WorkflowRunList(Vec<WorkflowRun>),
    WorkflowNodeRun(WorkflowNodeRun),
    WorkflowNodeRunChunks(Vec<WorkflowNodeRunChunk>),
    WorkflowNodeRunArtifacts(Vec<WorkflowNodeRunArtifact>),
    WorkflowRunArtifacts(Vec<WorkflowRunArtifact>),
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
    NodeBackends(NodeBackendsResponse),
    NodeGroup(ProvisionedGroup),
    NodeGroupList(Vec<ProvisionedGroup>),
    SecretBundle(SecretBundle),
    PackImport(PackImportResult),
    JsonValue(Value),
    JsonList(Vec<Value>),
    Notification(Notification),
    NotificationList(Vec<Notification>),
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowTriggerRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
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
pub struct RunStatusQuery {
    pub status: Option<RunStatus>,
}

#[derive(Debug, Deserialize)]
pub struct RunStatusRequest {
    pub status: RunStatus,
    #[serde(default)]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub active_node_id: Option<String>,
    #[serde(default)]
    pub state: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SchedulerTriggerClaimRequest {
    pub scheduler_id: String,
    #[serde(default)]
    pub limit: Option<i64>,
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
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunRequest {
    pub node_id: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub prev_node_run_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub attempt: Option<i64>,
    #[serde(default)]
    pub parameters: Option<Value>,
    #[serde(default)]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub state: Option<Value>,
    #[serde(default)]
    pub transition_reason: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunInputRequest {
    #[serde(default)]
    pub output_json: Value,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunExecutorClaimRequest {
    pub replica_id: Uuid,
    pub claimed_at: DateTime<Utc>,
    /// claims older than this are treated as stale and may be stolen; the worker derives it from the
    /// action timeout so a live executor is never preempted mid-run. defaults to `claimed_at` (steal
    /// only truly unclaimed slots) when an older worker omits it.
    #[serde(default)]
    pub stale_before: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunExecutorReleaseRequest {
    pub replica_id: Uuid,
    pub released_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
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
pub struct WorkflowOwnerRequest {
    /// the org to own the workflow, or `null`/absent to make it platform-global.
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct PipelineOwnerRequest {
    /// the org to own the pipeline, or `null`/absent to make it platform-global.
    #[serde(default)]
    pub org_id: Option<Uuid>,
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
    #[serde(alias = "secret")]
    pub value: Value,
    // declared json-schema, required once per config slot; ignored for secrets.
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub kind: SettingKind,
}

#[derive(Debug, Deserialize)]
pub struct WebhookWakeRequest {
    pub workflow_run_id: Uuid,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub message: Option<String>,
}
