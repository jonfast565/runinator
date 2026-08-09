use chrono::{DateTime, Utc};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::value::Value;
use runinator_models::{
    auth::{
        ApiKey, ApiKeyRecord, AuthSession, Grant, LocalCredential, Permission, PrincipalType,
        ResourceType, Team, User,
    },
    billing::{OrgQuota, OrgResourceGroup, UsageSample},
    errors::SendableError,
    notifications::{
        Notification, NotificationChannel, NotificationDelivery, NotificationDeliveryStatus,
        NotificationEvent, NotificationPolicy, NotificationSeverity,
    },
    orchestration::{OrchestrationEvent, ReadyNodeRecord},
    orgs::{OrgMembership, OrgRole, Organization},
    pipelines::{Pipeline, PipelineDefaults, PipelineRun, PipelineTrigger},
    provisioning::ProvisionBackend,
    replicas::{
        ReplicaKind, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus, TriggerActorType,
        TriggerSourceKind,
    },
    revisions::{RevisionSource, WorkflowRevision},
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    schedules::FreezeWindow,
    settings::{SettingKind, SettingRecord},
    telemetry::ReplicaSample,
    types::RuninatorType,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
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

fn parse_action_command(raw: String) -> Result<ActionCommand, SendableError> {
    serde_json::from_str::<ActionCommand>(&raw)
        .map_err(|err| crate::errors::ACTION_DISPATCH_INVALID_JSON.error(err))
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

mod core;
pub use core::*;
mod identity;
pub use identity::*;
mod workflows;
pub use workflows::*;
mod automation;
pub use automation::*;
mod engine;
pub use engine::*;
mod replicas;
pub use replicas::*;
mod notifications;
pub use notifications::*;

#[cfg(test)]
#[path = "mappers_tests.rs"]
mod mappers_tests;
