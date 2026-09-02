use chrono::{DateTime, Utc};
use runinator_comm::{AgentDirectiveKind, AgentDirectiveRecord, AgentDirectiveState};
use runinator_models::value::Value;
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::{
    auth::{
        AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, AuthSession, Grant,
        LocalCredential, Permission, PrincipalKind, PrincipalType, ResourceType, Team, User,
    },
    billing::{OrgQuota, OrgResourceGroup, UsageSample},
    console::{
        ConsoleBinding, ConsoleCell, ConsoleCellKind, ConsoleCellStatus, ConsoleFunction,
        ConsoleSession,
    },
    errors::SendableError,
    files::{FileDescriptor, FileScope, StoredFile},
    functions::{
        FunctionAdapterWorkflow, FunctionAlias, FunctionArtifact, FunctionCatalogEntry,
        FunctionExport, FunctionPackage, FunctionRuntimeSpec, FunctionVersion,
    },
    ingress_control::{
        BrokerIngressRecord, BrokerIngressSession, BrokerIngressSessionMode, ExternalIngressGate,
        ExternalIngressGateMode, ExternalIngressRecord, IngressControlState,
    },
    notifications::{
        Notification, NotificationChannel, NotificationDelivery, NotificationDeliveryStatus,
        NotificationEvent, NotificationPolicy, NotificationSeverity,
    },
    orchestration::{
        IngressAdmission, IngressAdmissionStatus, IngressEventDisposition, IngressInboxEntry,
        IngressQueueState, IngressTarget, IngressTargetKind,
    },
    orgs::{OrgMembership, OrgRole, Organization},
    pipelines::{
        Pipeline, PipelineDefaults, PipelineMemberAttempt, PipelineMemberAttemptStatus,
        PipelineRun, PipelineTrigger,
    },
    provisioning::ProvisionBackend,
    rbac::{ScopeKind, ScopeRef},
    replicas::{
        ReplicaKind, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus, TriggerActorType,
        TriggerSourceKind,
    },
    revisions::{PipelineRevision, RevisionSource, WorkflowRevision},
    schedules::FreezeWindow,
    settings::{SettingKind, SettingRecord},
    telemetry::ReplicaSample,
    types::RuninatorType,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowRun, WorkflowStatus, WorkflowTrigger,
        WorkflowTriggerKind,
    },
};
use sqlx::{ColumnIndex, Decode, Row, Type};
use uuid::Uuid;

fn parse_json(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

fn parse_type(raw: String) -> RuninatorType {
    let value = parse_json(raw);
    serde_json::from_value(value.clone().into())
        .unwrap_or_else(|_| RuninatorType::from_json_schema(&value))
}

/// define a row mapper generic over any sqlx row, with the column-decode bounds every mapper needs.
///
/// the `$row` identifier is supplied by the caller so the body and the generated signature share a
/// hygiene context. every column this codebase reads decodes as one of `i64`, `String`, `bool`,
/// `Uuid` (surrogate keys), `Option<i64>`, `Option<String>`, or `Option<Uuid>`, indexed by name.
macro_rules! row_mapper {
    ($name:ident($row:ident) -> $ret:ty $body:block) => {
        pub fn $name<R>($row: &R) -> $ret
        where
            R: Row,
            for<'c> &'c str: ColumnIndex<R>,
            for<'d> i64: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> String: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> bool: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Uuid: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<i64>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<String>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<Uuid>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Vec<u8>: Decode<'d, R::Database> + Type<R::Database>,
        $body
    };
}

macro_rules! fallible_row_mapper {
    ($name:ident($row:ident) -> $ret:ty $body:block) => {
        pub fn $name<R>($row: &R) -> Result<$ret, SendableError>
        where
            R: Row,
            for<'c> &'c str: ColumnIndex<R>,
            for<'d> i64: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> String: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> bool: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Uuid: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<i64>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<String>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Option<Uuid>: Decode<'d, R::Database> + Type<R::Database>,
            for<'d> Vec<u8>: Decode<'d, R::Database> + Type<R::Database>,
        $body
    };
}

mod core;
pub use core::*;
mod console;
pub use console::*;
mod files;
mod functions;
pub use files::*;
pub use functions::*;
mod workflow_vm;
pub use workflow_vm::*;
mod identity;
pub use identity::*;
mod workflows;
pub use workflows::*;
mod automation;
pub use automation::*;
mod replicas;
pub use replicas::*;
mod notifications;
pub use notifications::*;
mod ingress;
pub use ingress::*;
mod workspaces;
pub use workspaces::*;
mod orchestrations;
pub use orchestrations::*;

#[cfg(test)]
#[path = "mappers_tests.rs"]
mod mappers_tests;
