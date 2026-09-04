use super::*;

macro_rules! workflow_from_row {
    ($row:expr) => {{
        WorkflowDefinition {
            id: $row.get("id"),
            name: $row.get("name"),
            key: $row.get("resource_key"),
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

macro_rules! workflow_revision_from_row {
    ($row:expr) => {{
        WorkflowRevision {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            revision: $row.get("revision"),
            digest: $row.get("digest"),
            version: $row.get::<String, _>("version").parse().unwrap_or_default(),
            name: $row.get("name"),
            input_type: parse_type($row.get::<String, _>("input_schema")),
            definition: WorkflowGraph::from_value(parse_json($row.get::<String, _>("definition")))
                .unwrap_or_default(),
            source: RevisionSource::try_from($row.get::<String, _>("source").as_str())
                .unwrap_or_default(),
            actor_id: $row.get("actor_id"),
            actor_kind: $row.get("actor_kind"),
            note: $row.get::<Option<String>, _>("note"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
        }
    }};
}

row_mapper!(row_to_workflow_revision(row) -> WorkflowRevision { workflow_revision_from_row!(row) });

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

macro_rules! pipeline_from_row {
    ($row:expr) => {{
        Pipeline {
            id: $row.get("id"),
            name: $row.get("name"),
            key: $row.get("resource_key"),
            namespace: $row.get("namespace"),
            description: $row.get::<Option<String>, _>("description"),
            org_id: $row.get("org_id"),
            enabled: $row.get("enabled"),
            graph: serde_json::from_str($row.get::<String, _>("graph").as_str())
                .unwrap_or_default(),
            concurrency: serde_json::from_str($row.get::<String, _>("concurrency").as_str())
                .unwrap_or_default(),
            defaults: serde_json::from_str::<PipelineDefaults>(
                $row.get::<String, _>("defaults").as_str(),
            )
            .unwrap_or_default(),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0),
        }
    }};
}

row_mapper!(row_to_pipeline(row) -> Pipeline { pipeline_from_row!(row) });

macro_rules! pipeline_revision_from_row {
    ($row:expr) => {{
        PipelineRevision {
            id: $row.get("id"),
            pipeline_id: $row.get("pipeline_id"),
            revision: $row.get("revision"),
            digest: $row.get("digest"),
            name: $row.get("name"),
            description: $row.get("description"),
            graph: serde_json::from_str($row.get::<String, _>("graph").as_str())
                .unwrap_or_default(),
            concurrency: serde_json::from_str($row.get::<String, _>("concurrency").as_str())
                .unwrap_or_default(),
            defaults: serde_json::from_str($row.get::<String, _>("defaults").as_str())
                .unwrap_or_default(),
            metadata: parse_json($row.get::<String, _>("metadata")),
            source: RevisionSource::try_from($row.get::<String, _>("source").as_str())
                .unwrap_or_default(),
            actor_id: $row.get("actor_id"),
            actor_kind: $row.get("actor_kind"),
            note: $row.get("note"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
        }
    }};
}

row_mapper!(row_to_pipeline_revision(row) -> PipelineRevision { pipeline_revision_from_row!(row) });

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
            execution_state: WorkflowExecutionState::default(),
            state_version: $row.try_get("state_version").unwrap_or(0),
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
            correlation_key: $row.try_get("correlation_key").ok().flatten(),
            pipeline_run_id: $row.try_get("pipeline_run_id").ok().flatten(),
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

macro_rules! pipeline_trigger_from_row {
    ($row:expr) => {{
        PipelineTrigger {
            id: $row.get("id"),
            pipeline_id: $row.get("pipeline_id"),
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

row_mapper!(row_to_pipeline_trigger(row) -> PipelineTrigger { pipeline_trigger_from_row!(row) });

macro_rules! pipeline_run_from_row {
    ($row:expr) => {{
        PipelineRun {
            id: $row.get("id"),
            pipeline_id: $row.get("pipeline_id"),
            pipeline_snapshot: $row
                .get::<Option<String>, _>("pipeline_snapshot")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
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
            trigger_metadata: $row
                .try_get::<String, _>("trigger_metadata")
                .map(parse_json)
                .unwrap_or(Value::Null),
            orchestration_binding_id: $row.try_get("orchestration_binding_id").ok().flatten(),
            execution_epoch: $row.try_get("execution_epoch").ok().flatten(),
            start_member: $row.try_get("start_member").ok().flatten(),
        }
    }};
}

row_mapper!(row_to_pipeline_run(row) -> PipelineRun { pipeline_run_from_row!(row) });

row_mapper!(row_to_pipeline_member_attempt(row) -> PipelineMemberAttempt {
    PipelineMemberAttempt {
        id: row.get("id"),
        pipeline_run_id: row.get("pipeline_run_id"),
        member_key: row.get("member_key"),
        workflow_id: row.get("workflow_id"),
        attempt: row.get("attempt"),
        workflow_run_id: row.get("workflow_run_id"),
        status: PipelineMemberAttemptStatus::try_from(row.get::<String, _>("status").as_str())
            .unwrap_or(PipelineMemberAttemptStatus::Failed),
        parameters: parse_json(row.get::<String, _>("parameters")),
        result: parse_json(row.get::<String, _>("result")),
        message: row.get("message"),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        started_at: row.get::<Option<i64>, _>("started_at").and_then(|v| DateTime::<Utc>::from_timestamp(v, 0)),
        finished_at: row.get::<Option<i64>, _>("finished_at").and_then(|v| DateTime::<Utc>::from_timestamp(v, 0)),
    }
});
