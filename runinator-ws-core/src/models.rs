use chrono::{DateTime, Utc};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord};
use runinator_models::value::Value;
use runinator_models::{
    ai_usage::AiUsageReport,
    bundles::{PackImportResult, ProviderBundle},
    console::{ConsoleCell, ConsoleSession, ConsoleSessionDetail},
    execution_profiles::{
        ExecutionProfile, ExecutionProfileCollectionStatus, ExecutionProfileOperation,
        ExecutionProfileRevision,
    },
    files::StoredFile,
    functions::{
        FunctionAlias, FunctionArtifact, FunctionCatalogEntry, FunctionInvocationTarget,
        FunctionPackage, FunctionPackageDetail, FunctionVersion,
    },
    ingress_control::{
        BrokerIngressRecord, BrokerIngressSession, BrokerMessageRecord, ExternalIngressGate,
        ExternalIngressRecord,
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

use runinator_models::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, bounded_text, identifier,
    optional_text, positive_limit, required_text,
};

#[derive(Serialize)]
#[serde(untagged)]
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
    ExternalIngressGate(ExternalIngressGate),
    ExternalIngressRecord(ExternalIngressRecord),
    ExternalIngressRecordList(Vec<ExternalIngressRecord>),
    BrokerIngressSession(BrokerIngressSession),
    BrokerIngressRecord(BrokerIngressRecord),
    BrokerIngressRecordList(Vec<BrokerIngressRecord>),
    BrokerMessageList(Vec<BrokerMessageRecord>),
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
    WorkflowRun(Box<WorkflowRunResponse>),
    WorkflowRunList(Vec<WorkflowRun>),
    AiUsageReport(AiUsageReport),
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
    ExecutionProfile(ExecutionProfile),
    ExecutionProfileList(Vec<ExecutionProfile>),
    ExecutionProfileCollectionStatusList(Vec<ExecutionProfileCollectionStatus>),
    ExecutionProfileOperationList(Vec<ExecutionProfileOperation>),
    ExecutionProfileOperation(ExecutionProfileOperation),
    ExecutionProfileRevision(ExecutionProfileRevision),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunInquiryDecision {
    Continue,
    Abort,
}

mod api_error;
pub use api_error::ApiError;

mod auth_config_response_schema;
pub use auth_config_response_schema::AuthConfigResponseSchema;

mod login_request_schema;
pub use login_request_schema::LoginRequestSchema;

mod user_schema;
pub use user_schema::UserSchema;

mod login_response_schema;
pub use login_response_schema::LoginResponseSchema;

mod refresh_request_schema;
pub use refresh_request_schema::RefreshRequestSchema;

mod task_response_schema;
pub use task_response_schema::TaskResponseSchema;

mod create_agent_directive_request;
pub use create_agent_directive_request::CreateAgentDirectiveRequest;

mod agent_directive_query;
pub use agent_directive_query::AgentDirectiveQuery;

mod workflow_run_request;
pub use workflow_run_request::WorkflowRunRequest;

mod workflow_trigger_run_request;
pub use workflow_trigger_run_request::WorkflowTriggerRunRequest;

mod pipeline_run_request;
pub use pipeline_run_request::PipelineRunRequest;

mod orchestration_intent_request;
pub use orchestration_intent_request::OrchestrationIntentRequest;

mod orchestration_requeue_request;
pub use orchestration_requeue_request::OrchestrationRequeueRequest;

mod adapter_apply_request;
pub use adapter_apply_request::AdapterApplyRequest;

mod adapter_enable_request;
pub use adapter_enable_request::AdapterEnableRequest;

mod adapter_test_request;
pub use adapter_test_request::AdapterTestRequest;

mod adapter_draft_test_request;
pub use adapter_draft_test_request::AdapterDraftTestRequest;

mod external_operation_resolution_request;
pub use external_operation_resolution_request::ExternalOperationResolutionRequest;

mod ingress_event_request;
pub use ingress_event_request::IngressEventRequest;

mod ingress_admission_query;
pub use ingress_admission_query::IngressAdmissionQuery;

mod ingress_response;
pub use ingress_response::IngressResponse;

mod managed_run_override_request;
pub use managed_run_override_request::ManagedRunOverrideRequest;

mod pipeline_member_retry_request;
pub use pipeline_member_retry_request::PipelineMemberRetryRequest;

mod setting_move_request;
pub use setting_move_request::SettingMoveRequest;

mod pipeline_run_resolution_request;
pub use pipeline_run_resolution_request::PipelineRunResolutionRequest;

mod workflow_run_status_query;
pub use workflow_run_status_query::WorkflowRunStatusQuery;

mod workflow_run_status_request;
pub use workflow_run_status_request::WorkflowRunStatusRequest;

mod scheduler_run_claim_request;
pub use scheduler_run_claim_request::SchedulerRunClaimRequest;

mod scheduler_run_claim_renew_request;
pub use scheduler_run_claim_renew_request::SchedulerRunClaimRenewRequest;

mod scheduler_run_claim_release_request;
pub use scheduler_run_claim_release_request::SchedulerRunClaimReleaseRequest;

mod workflow_run_rename_request;
pub use workflow_run_rename_request::WorkflowRunRenameRequest;

mod signal_delivery_request;
pub use signal_delivery_request::SignalDeliveryRequest;

mod interrupt_request;
pub use interrupt_request::InterruptRequest;

mod event_delivery_request;
pub use event_delivery_request::EventDeliveryRequest;

mod webhook_signal_request;
pub use webhook_signal_request::WebhookSignalRequest;

mod workflow_run_replay_request;
pub use workflow_run_replay_request::WorkflowRunReplayRequest;

mod workflow_run_response;
pub use workflow_run_response::WorkflowRunResponse;

mod catalog_query;
pub use catalog_query::CatalogQuery;

mod automation_record_query;
pub use automation_record_query::AutomationRecordQuery;

mod approval_resolution_request;
pub use approval_resolution_request::ApprovalResolutionRequest;

mod gate_query;
pub use gate_query::GateQuery;

mod dead_letter_query;
pub use dead_letter_query::DeadLetterQuery;

mod broker_message_query;
pub use broker_message_query::BrokerMessageQuery;

mod audit_log_query;
pub use audit_log_query::AuditLogQuery;

mod gate_resolution_request;
pub use gate_resolution_request::GateResolutionRequest;

mod idempotency_request;
pub use idempotency_request::IdempotencyRequest;

mod credential_query;
pub use credential_query::CredentialQuery;

mod replica_query;
pub use replica_query::ReplicaQuery;

mod replica_sample_query;
pub use replica_sample_query::ReplicaSampleQuery;

mod credential_put_request;
pub use credential_put_request::CredentialPutRequest;
