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
    notifications::Notification,
    orchestration::{OrchestrationEvent, ReadyNodeRecord},
    orgs::{OrgMembership, OrgRole, Organization},
    provisioning::ProvisionBackend,
    replicas::{
        ReplicaKind, ReplicaProviderRegistration, ReplicaRecord, ReplicaStatus, TriggerActorType,
        TriggerSourceKind,
    },
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
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

macro_rules! run_summary_from_row {
    ($row:expr) => {{
        RunSummary {
            id: $row.get("id"),
            status: RunStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(RunStatus::Failed),
            parameters: parse_json($row.get::<String, _>("parameters")),
            output_json: $row
                .get::<Option<String>, _>("output_json")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            message: $row.get("message"),
            trigger: $row.get("trigger"),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            workflow_run_id: $row.get("workflow_run_id"),
            workflow_node_id: $row.get("workflow_node_id"),
        }
    }};
}

row_mapper!(row_to_run_summary(row) -> RunSummary { run_summary_from_row!(row) });

macro_rules! setting_from_row {
    ($row:expr) => {{
        SettingRecord {
            kind: SettingKind::from_str_lossy(&$row.get::<String, _>("kind")),
            scope: $row.get("scope"),
            name: $row.get("name"),
            value: $row.get("value"),
            updated_at: $row.get("updated_at"),
        }
    }};
}

row_mapper!(row_to_setting(row) -> SettingRecord { setting_from_row!(row) });

macro_rules! user_from_row {
    ($row:expr) => {
        User {
            id: Some($row.get::<Uuid, _>("id")),
            username: $row.get::<String, _>("username"),
            email: $row.get::<Option<String>, _>("email"),
            is_admin: $row.get::<bool, _>("is_admin"),
            disabled: $row.get::<bool, _>("disabled"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    };
}

macro_rules! api_key_from_row {
    ($row:expr) => {
        ApiKey {
            id: Some($row.get::<Uuid, _>("id")),
            name: $row.get::<String, _>("name"),
            user_id: $row.get::<Option<Uuid>, _>("user_id"),
            is_service: $row.get::<bool, _>("is_service"),
            key_prefix: $row.get::<String, _>("key_prefix"),
            last_used_at: $row
                .get::<Option<i64>, _>("last_used_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            expires_at: $row
                .get::<Option<i64>, _>("expires_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            disabled: $row.get::<bool, _>("disabled"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    };
}

row_mapper!(row_to_user(row) -> User { user_from_row!(row) });

row_mapper!(row_to_local_credential(row) -> LocalCredential {
    LocalCredential {
        user: user_from_row!(row),
        password_hash: row.get::<String, _>("password_hash"),
    }
});

row_mapper!(row_to_api_key(row) -> ApiKey { api_key_from_row!(row) });

row_mapper!(row_to_api_key_record(row) -> ApiKeyRecord {
    ApiKeyRecord {
        key: api_key_from_row!(row),
        is_admin: row.get::<bool, _>("is_admin"),
        key_hash: row.get::<String, _>("key_hash"),
    }
});

row_mapper!(row_to_auth_session(row) -> AuthSession {
    AuthSession {
        id: row.get::<Uuid, _>("id"),
        user_id: row.get::<Uuid, _>("user_id"),
        refresh_token_hash: row.get::<String, _>("refresh_token_hash"),
        expires_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("expires_at"), 0)
            .unwrap_or_else(Utc::now),
        revoked: row.get::<bool, _>("revoked"),
    }
});

row_mapper!(row_to_team(row) -> Team {
    Team {
        id: Some(row.get::<Uuid, _>("id")),
        name: row.get::<String, _>("name"),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_organization(row) -> Organization {
    Organization {
        id: Some(row.get::<Uuid, _>("id")),
        name: row.get::<String, _>("name"),
        slug: row.get::<String, _>("slug"),
        disabled: row.get::<bool, _>("disabled"),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("updated_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_membership(row) -> OrgMembership {
    OrgMembership {
        org_id: row.get::<Uuid, _>("org_id"),
        user_id: row.get::<Uuid, _>("user_id"),
        role: OrgRole::from_str_lossy(&row.get::<String, _>("role")).unwrap_or(OrgRole::Member),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_quota(row) -> OrgQuota {
    OrgQuota {
        org_id: row.get::<Uuid, _>("org_id"),
        max_nodes_per_kind: serde_json::from_str(&row.get::<String, _>("max_nodes_json"))
            .unwrap_or_default(),
        max_monthly_cents: row.get::<i64, _>("max_monthly_cents") as u32,
    }
});

row_mapper!(row_to_usage_sample(row) -> UsageSample {
    UsageSample {
        org_id: row.get::<Uuid, _>("org_id"),
        backend: ProvisionBackend::try_from(row.get::<String, _>("backend").as_str())
            .unwrap_or(ProvisionBackend::Supervisor),
        kind: ReplicaKind::try_from(row.get::<String, _>("kind").as_str())
            .unwrap_or(ReplicaKind::Worker),
        node_count: row.get::<i64, _>("node_count") as u32,
        sampled_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("sampled_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_resource_group(row) -> OrgResourceGroup {
    OrgResourceGroup {
        org_id: row.get::<Uuid, _>("org_id"),
        backend: ProvisionBackend::try_from(row.get::<String, _>("backend").as_str())
            .unwrap_or(ProvisionBackend::Supervisor),
        kind: ReplicaKind::try_from(row.get::<String, _>("kind").as_str())
            .unwrap_or(ReplicaKind::Worker),
        desired: row.get::<i64, _>("desired") as u32,
        dedicated: row.get::<bool, _>("dedicated"),
    }
});

row_mapper!(row_to_grant(row) -> Grant {
    Grant {
        id: Some(row.get::<Uuid, _>("id")),
        // only `workflow` exists today; the column is stored for forward compatibility.
        resource_type: ResourceType::Workflow,
        resource_id: row.get::<Uuid, _>("resource_id"),
        principal_type: PrincipalType::from_str_lossy(&row.get::<String, _>("principal_type"))
            .unwrap_or(PrincipalType::User),
        principal_id: row.get::<Uuid, _>("principal_id"),
        permission: Permission::from_str_lossy(&row.get::<String, _>("permission"))
            .unwrap_or(Permission::View),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

macro_rules! run_chunk_from_row {
    ($row:expr) => {{
        RunChunk {
            id: $row.get("id"),
            run_id: $row.get("run_id"),
            sequence: $row.get("sequence"),
            stream: $row.get("stream"),
            content: $row.get("content"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_run_chunk(row) -> RunChunk { run_chunk_from_row!(row) });

macro_rules! run_artifact_from_row {
    ($row:expr) => {{
        RunArtifact {
            id: $row.get("id"),
            run_id: $row.get("run_id"),
            name: $row.get("name"),
            mime_type: $row.get("mime_type"),
            size_bytes: $row.get("size_bytes"),
            uri: $row.get("uri"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_run_artifact(row) -> RunArtifact { run_artifact_from_row!(row) });

macro_rules! workflow_from_row {
    ($row:expr) => {{
        WorkflowDefinition {
            id: $row.get("id"),
            name: $row.get("name"),
            namespace: $row.get("namespace"),
            org_id: $row.get("org_id"),
            version: $row.get::<String, _>("version").parse().unwrap_or_default(),
            enabled: $row.get("enabled"),
            input_type: parse_type($row.get::<String, _>("input_schema")),
            definition: WorkflowGraph::from_value(parse_json($row.get::<String, _>("definition")))
                .unwrap_or_default(),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0),
        }
    }};
}

row_mapper!(row_to_workflow(row) -> WorkflowDefinition { workflow_from_row!(row) });

macro_rules! workflow_trigger_from_row {
    ($row:expr) => {{
        WorkflowTrigger {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            kind: WorkflowTriggerKind::try_from($row.get::<String, _>("kind").as_str())
                .unwrap_or(WorkflowTriggerKind::Manual),
            enabled: $row.get("enabled"),
            configuration: parse_json($row.get::<String, _>("configuration")),
            next_execution: $row
                .get::<Option<i64>, _>("next_execution")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            blackout_start: $row
                .get::<Option<i64>, _>("blackout_start")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            blackout_end: $row
                .get::<Option<i64>, _>("blackout_end")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0),
        }
    }};
}

row_mapper!(row_to_workflow_trigger(row) -> WorkflowTrigger { workflow_trigger_from_row!(row) });

macro_rules! workflow_run_from_row {
    ($row:expr) => {{
        WorkflowRun {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            workflow_snapshot: $row
                .get::<Option<String>, _>("workflow_snapshot")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            active_node_id: $row.get("active_node_id"),
            parameters: parse_json($row.get::<String, _>("parameters")),
            state: parse_json($row.get::<String, _>("state")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            message: $row.get("message"),
            name: $row.get("name"),
            trigger_source_kind: $row
                .try_get::<Option<String>, _>("trigger_source_kind")
                .ok()
                .flatten()
                .as_deref()
                .map(TriggerSourceKind::try_from)
                .transpose()
                .ok()
                .flatten(),
            trigger_actor_type: $row
                .try_get::<Option<String>, _>("trigger_actor_type")
                .ok()
                .flatten()
                .as_deref()
                .map(TriggerActorType::try_from)
                .transpose()
                .ok()
                .flatten(),
            trigger_actor_replica_id: $row.try_get("trigger_actor_replica_id").ok().flatten(),
            trigger_actor_display_name: $row.try_get("trigger_actor_display_name").ok().flatten(),
            trigger_request_host: $row.try_get("trigger_request_host").ok().flatten(),
            trigger_request_ip: $row.try_get("trigger_request_ip").ok().flatten(),
            trigger_metadata: $row
                .try_get::<String, _>("trigger_metadata")
                .map(parse_json)
                .unwrap_or(Value::Null),
        }
    }};
}

row_mapper!(row_to_workflow_run(row) -> WorkflowRun { workflow_run_from_row!(row) });

macro_rules! workflow_node_run_from_row {
    ($row:expr) => {{
        WorkflowNodeRun {
            id: $row.get("id"),
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            attempt: $row.get("attempt"),
            parameters: parse_json($row.get::<String, _>("parameters")),
            output_json: $row
                .get::<Option<String>, _>("output_json")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            state: parse_json($row.get::<String, _>("state")),
            transition_reason: $row.get("transition_reason"),
            prev_node_run_id: $row.try_get("prev_node_run_id").ok().flatten(),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            message: $row.get("message"),
            current_executor_replica_id: $row.try_get("current_executor_replica_id").ok().flatten(),
            last_executor_replica_id: $row.try_get("last_executor_replica_id").ok().flatten(),
            executor_claimed_at: $row
                .try_get::<Option<i64>, _>("executor_claimed_at")
                .ok()
                .flatten()
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            executor_released_at: $row
                .try_get::<Option<i64>, _>("executor_released_at")
                .ok()
                .flatten()
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        }
    }};
}

row_mapper!(row_to_workflow_node_run(row) -> WorkflowNodeRun { workflow_node_run_from_row!(row) });

macro_rules! workflow_node_run_chunk_from_row {
    ($row:expr) => {{
        WorkflowNodeRunChunk {
            id: $row.get("id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            sequence: $row.get("sequence"),
            stream: $row.get("stream"),
            content: $row.get("content"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_workflow_node_run_chunk(row) -> WorkflowNodeRunChunk {
    workflow_node_run_chunk_from_row!(row)
});

macro_rules! workflow_node_run_artifact_from_row {
    ($row:expr) => {{
        WorkflowNodeRunArtifact {
            id: $row.get("id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            name: $row.get("name"),
            mime_type: $row.get("mime_type"),
            size_bytes: $row.get("size_bytes"),
            uri: $row.get("uri"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_workflow_node_run_artifact(row) -> WorkflowNodeRunArtifact {
    workflow_node_run_artifact_from_row!(row)
});

macro_rules! workflow_run_artifact_from_row {
    ($row:expr) => {{
        WorkflowRunArtifact {
            id: $row.get("id"),
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            artifact_id: $row.get("artifact_id"),
            name: $row.get("name"),
            mime_type: $row.get("mime_type"),
            size_bytes: $row.get("size_bytes"),
            uri: $row.get("uri"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_workflow_run_artifact(row) -> WorkflowRunArtifact {
    workflow_run_artifact_from_row!(row)
});

macro_rules! catalog_item_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "uri": $row.get::<String, _>("uri"),
            "item_type": $row.get::<String, _>("item_type"),
            "name": $row.get::<String, _>("name"),
            "version": $row.get::<String, _>("version"),
            "document": parse_json($row.get::<String, _>("document")),
            "metadata": parse_json($row.get::<String, _>("metadata")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0).unwrap_or_else(Utc::now),
            "updated_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0).unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_catalog_item(row) -> Value { catalog_item_from_row!(row) });

macro_rules! automation_record_from_row {
    ($row:expr) => {{
        let mut data = parse_json($row.get::<String, _>("data"));
        if !data.is_object() {
            data = Value::Object(Default::default());
        }
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "id".into(),
                Value::from($row.get::<Uuid, _>("id").to_string()),
            );
            object.insert(
                "record_type".into(),
                Value::from($row.get::<String, _>("record_type")),
            );
            object.insert(
                "created_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
            object.insert(
                "updated_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
        }
        data
    }};
}

row_mapper!(row_to_automation_record(row) -> Value { automation_record_from_row!(row) });

macro_rules! gate_from_row {
    ($row:expr) => {{
        let mut data = parse_json($row.get::<String, _>("data"));
        if !data.is_object() {
            data = Value::Object(Default::default());
        }
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "id".into(),
                Value::from($row.get::<Uuid, _>("id").to_string()),
            );
            object.insert(
                "created_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
            object.insert(
                "updated_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
        }
        data
    }};
}

row_mapper!(row_to_gate(row) -> Value { gate_from_row!(row) });

macro_rules! dead_letter_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "channel": $row.get::<String, _>("channel"),
            "event_id": $row.get::<Option<Uuid>, _>("event_id"),
            "dedupe_key": $row.get::<Option<String>, _>("dedupe_key"),
            "attempts": $row.get::<i64, _>("attempts"),
            "error": $row.get::<String, _>("error"),
            "payload": parse_json($row.get::<String, _>("payload")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_dead_letter(row) -> Value { dead_letter_from_row!(row) });

macro_rules! audit_log_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "actor_id": $row.get::<Option<Uuid>, _>("actor_id"),
            "actor_kind": $row.get::<String, _>("actor_kind"),
            "action": $row.get::<String, _>("action"),
            "resource_type": $row.get::<Option<String>, _>("resource_type"),
            "resource_id": $row.get::<Option<Uuid>, _>("resource_id"),
            "outcome": $row.get::<String, _>("outcome"),
            "detail": $row.get::<Option<String>, _>("detail"),
            "metadata": parse_json($row.get::<String, _>("metadata")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_audit_log(row) -> Value { audit_log_from_row!(row) });

macro_rules! idempotency_key_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "scope": $row.get::<String, _>("scope"),
            "key": $row.get::<String, _>("key"),
            "result": parse_json($row.get::<String, _>("result")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0).unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_idempotency_key(row) -> Value { idempotency_key_from_row!(row) });

macro_rules! action_dispatch_from_row {
    ($row:expr) => {{
        let raw = $row.get::<String, _>("command_json");
        Ok(ActionDispatchRecord {
            id: $row.get("id"),
            dedupe_key: $row.get("dedupe_key"),
            command: parse_action_command(raw)?,
            attempts: $row.get("attempts"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
            published_at: $row
                .get::<Option<i64>, _>("published_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            last_error: $row.get("last_error"),
            claimed_by: $row
                .try_get::<Option<String>, _>("claimed_by")
                .ok()
                .flatten(),
            claimed_until: $row
                .try_get::<Option<i64>, _>("claimed_until")
                .ok()
                .flatten()
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        })
    }};
}

row_mapper!(row_to_action_dispatch(row) -> Result<ActionDispatchRecord, SendableError> {
    action_dispatch_from_row!(row)
});

macro_rules! orchestration_event_from_row {
    ($row:expr) => {{
        Ok(OrchestrationEvent {
            event_id: $row.get::<Uuid, _>("event_id"),
            workflow_run_id: $row.get("workflow_run_id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            node_id: $row.get("node_id"),
            event_type: $row.get("event_type"),
            payload: parse_json($row.get::<String, _>("payload")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_orchestration_event(row) -> Result<OrchestrationEvent, SendableError> {
    orchestration_event_from_row!(row)
});

macro_rules! ready_node_from_row {
    ($row:expr) => {{
        Ok(ReadyNodeRecord {
            id: $row.get("id"),
            source_event_id: $row.get::<Uuid, _>("source_event_id"),
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            ready_at: DateTime::<Utc>::from_timestamp($row.get("ready_at"), 0)
                .unwrap_or_else(Utc::now),
            attempts: $row.get("attempts"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
            claimed_by: $row.get("claimed_by"),
            claimed_until: $row
                .get::<Option<i64>, _>("claimed_until")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            completed_at: $row
                .get::<Option<i64>, _>("completed_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        })
    }};
}

row_mapper!(row_to_ready_node(row) -> Result<ReadyNodeRecord, SendableError> {
    ready_node_from_row!(row)
});

macro_rules! replica_from_row {
    ($row:expr) => {{
        Ok(ReplicaRecord {
            replica_id: $row.get("replica_id"),
            replica_type: ReplicaKind::try_from($row.get::<String, _>("replica_type").as_str())
                .unwrap_or(ReplicaKind::Worker),
            instance_id: $row.get("instance_id"),
            runtime_id: $row.get("runtime_id"),
            status: ReplicaStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(ReplicaStatus::Offline),
            display_name: $row.get("display_name"),
            host: $row.get("host"),
            port: $row
                .get::<Option<i64>, _>("port")
                .and_then(|value| u16::try_from(value).ok()),
            base_path: $row.get("base_path"),
            observed_ip: $row.get("observed_ip"),
            version: $row.get("version"),
            attributes: parse_json($row.get::<String, _>("attributes")),
            first_seen_at: DateTime::<Utc>::from_timestamp($row.get("first_seen_at"), 0)
                .unwrap_or_else(Utc::now),
            last_heartbeat_at: DateTime::<Utc>::from_timestamp($row.get("last_heartbeat_at"), 0)
                .unwrap_or_else(Utc::now),
            last_seen_at: DateTime::<Utc>::from_timestamp($row.get("last_seen_at"), 0)
                .unwrap_or_else(Utc::now),
            offline_at: $row
                .get::<Option<i64>, _>("offline_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            registered_by_principal_id: $row.get("registered_by_principal_id"),
            registered_by_kind: $row.get("registered_by_kind"),
            registered_by_org_id: $row.get("registered_by_org_id"),
        })
    }};
}

row_mapper!(row_to_replica(row) -> Result<ReplicaRecord, SendableError> {
    replica_from_row!(row)
});

row_mapper!(row_to_replica_sample(row) -> ReplicaSample {
    // the sample's numeric fields ride in a json `data` column (avoiding typed float columns), with
    // replica_id/sampled_at duplicated as typed columns for indexing and pruning.
    serde_json::from_str::<ReplicaSample>(&row.get::<String, _>("data")).unwrap_or_else(|_| {
        ReplicaSample {
            replica_id: row.get("replica_id"),
            sampled_at: DateTime::<Utc>::from_timestamp(row.get("sampled_at"), 0)
                .unwrap_or_else(Utc::now),
            cpu_percent: 0.0,
            mem_percent: 0.0,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
            load_one: None,
            process_cpu_percent: 0.0,
            process_mem_bytes: 0,
            net_rx_bytes_per_sec: 0.0,
            net_tx_bytes_per_sec: 0.0,
        }
    })
});

macro_rules! replica_provider_registration_from_row {
    ($row:expr) => {{
        Ok(ReplicaProviderRegistration {
            replica_id: $row.get("replica_id"),
            provider_name: $row.get("provider_name"),
            provider: serde_json::from_str(&$row.get::<String, _>("provider_json")).unwrap_or(
                runinator_models::providers::ProviderMetadata {
                    name: $row.get("provider_name"),
                    actions: Vec::new(),
                    metadata: Default::default(),
                },
            ),
            first_registered_at: DateTime::<Utc>::from_timestamp(
                $row.get("first_registered_at"),
                0,
            )
            .unwrap_or_else(Utc::now),
            last_registered_at: DateTime::<Utc>::from_timestamp($row.get("last_registered_at"), 0)
                .unwrap_or_else(Utc::now),
            last_heartbeat_at: DateTime::<Utc>::from_timestamp($row.get("last_heartbeat_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_replica_provider_registration(row) -> Result<ReplicaProviderRegistration, SendableError> {
    replica_provider_registration_from_row!(row)
});

macro_rules! notification_from_row {
    ($row:expr) => {{
        Notification {
            id: $row.get::<Uuid, _>("id"),
            workflow_run_id: $row.get::<Option<Uuid>, _>("workflow_run_id"),
            workflow_node_id: $row.get::<Option<String>, _>("workflow_node_id"),
            channel: $row.get::<String, _>("channel"),
            severity: $row.get::<String, _>("severity"),
            title: $row.get::<String, _>("title"),
            body: $row.get::<Option<String>, _>("body"),
            target: $row.get::<Option<String>, _>("target"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            read_at: $row
                .get::<Option<i64>, _>("read_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_notification(row) -> Notification { notification_from_row!(row) });

#[cfg(test)]
#[path = "mappers_tests.rs"]
mod tests;
