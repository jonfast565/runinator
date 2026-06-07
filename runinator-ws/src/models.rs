use chrono::{DateTime, Utc};
use runinator_models::value::Value;
use runinator_models::{
    bundles::{PackImportResult, ProviderBundle, SecretBundle},
    notifications::Notification,
    providers::ProviderMetadata,
    replicas::{
        ReplicaListResponse, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus,
    },
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    settings::SettingKind,
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
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
    WorkflowRun(WorkflowRunResponse),
    WorkflowRunList(Vec<WorkflowRun>),
    WorkflowNodeRun(WorkflowNodeRun),
    WorkflowNodeRunChunks(Vec<WorkflowNodeRunChunk>),
    WorkflowNodeRunArtifacts(Vec<WorkflowNodeRunArtifact>),
    Provider(ProviderMetadata),
    ProviderList(Vec<ProviderMetadata>),
    ProviderBundle(ProviderBundle),
    Replica(ReplicaRecord),
    ReplicaList(ReplicaListResponse),
    ReplicaProviderRegistration(ReplicaProviderRegistration),
    ReplicaProviderRegistrationList(Vec<ReplicaProviderRegistration>),
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
    pub workflow_id: Option<i64>,
    pub name: Option<String>,
    pub open: Option<bool>,
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

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRenameRequest {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct WorkflowRunReplayRequest {
    #[serde(default)]
    pub from_step_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunRequest {
    pub node_id: String,
    #[serde(default)]
    pub parameters: Value,
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
pub struct WorkflowNodeRunExecutorClaimRequest {
    pub replica_id: i64,
    pub claimed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunExecutorReleaseRequest {
    pub replica_id: i64,
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
    pub workflow_run_id: Option<i64>,
    pub external_item_id: Option<i64>,
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
    pub workflow_run_id: i64,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub message: Option<String>,
}
