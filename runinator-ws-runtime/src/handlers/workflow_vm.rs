//! Operator reads for the compiled workflow VM.
//!
//! These intentionally expose durable continuations, effects, and journal records directly.
//! A VM-backed run must never reconstruct its history from legacy node-run rows.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use axum::{
    Extension, Json,
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_broker_core::Broker;
use runinator_comm::ControlCommand;
use runinator_engine::services::WorkflowFiles;
use runinator_models::{
    auth::{AuthContext, Permission},
    orchestration::{NodeTransition, NodeTransitionStat},
    runs::ProviderTerminalControl,
    validation::{LONG_TEXT_MAX, Validate, ValidationError, optional_text},
    value::Value,
    web::TaskResponse,
    workflow_vm::WorkflowVmCursor,
    workflow_vm::{
        WorkflowEffect, WorkflowEffectStatus, WorkflowJournalEntry, WorkflowJournalRecord,
    },
};
use runinator_store::{
    RuntimeStore,
    roles::{FileStore, WorkflowVmStore},
};
use serde::Deserialize;
use uuid::Uuid;

use runinator_ws_core::{
    ValidatedJson,
    events::{EventSender, emit_workflow_run},
    models::ApiResponse,
    openapi::docs::{EndpointDoc, Example, endpoint},
    responses::{api_error, not_found},
};
use runinator_ws_middleware::authz::{AuthorizationStore, AuthzChecker};

async fn authorize_run<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    db: &T,
    ctx: &AuthContext,
    workflow_run_id: Uuid,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    AuthzChecker::new(db, ctx)
        .require_run_workflow(workflow_run_id, Permission::View)
        .await
}

pub async fn list_continuations<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_continuations(workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowContinuationList(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_continuation<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(continuation_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.fetch_workflow_continuation(continuation_id).await {
        Ok(Some(record)) => {
            if let Err(reply) = authorize_run(db.as_ref(), &ctx, record.workflow_run_id).await {
                return reply;
            }
            (
                StatusCode::OK,
                Json(ApiResponse::WorkflowContinuation(record)),
            )
        }
        Ok(None) => not_found(format!("workflow continuation {continuation_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_effects<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match project_effect_nodes(db.as_ref(), workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowEffectList(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// Effects outlive the continuation location that issued them. Project their immutable journal
/// boundary through the pinned module so operator clients can keep historical node highlights.
async fn project_effect_nodes<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Vec<WorkflowEffect>, runinator_models::errors::SendableError> {
    let (mut effects, journal, module) = tokio::try_join!(
        db.fetch_workflow_effects(workflow_run_id),
        db.fetch_workflow_journal(workflow_run_id),
        db.fetch_workflow_module(workflow_run_id),
    )?;
    let Some(module) = module else {
        return Ok(effects);
    };
    let mut node_by_effect = journal
        .into_iter()
        .filter_map(|record| match record.entry {
            WorkflowJournalEntry::EffectRequested {
                effect_id,
                instruction_pointer: Some(instruction_pointer),
            } => module
                .graph_location(instruction_pointer)
                .map(|location| (effect_id, location.node_id.clone())),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    // Runs created before EffectRequested carried its instruction pointer still have the frozen
    // module and immutable effect request. If that request occurs in exactly one graph node, it is
    // a safe historical source-map fallback. Ambiguous duplicate requests deliberately remain
    // unassigned instead of being painted onto the continuation's final cursor.
    let legacy_candidates = module
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(instruction_pointer, instruction)| match instruction {
            runinator_models::workflow_vm::WorkflowInstruction::Effect { request } => module
                .graph_location(instruction_pointer)
                .map(|location| (request, location.node_id.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    for effect in &effects {
        if node_by_effect.contains_key(&effect.id) {
            continue;
        }
        let candidates = legacy_candidates
            .iter()
            .filter_map(|(request, node_id)| {
                legacy_effect_call_site_matches(request, &effect.request).then_some(*node_id)
            })
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(node_id) = candidates
            .iter()
            .copied()
            .next()
            .filter(|_| candidates.len() == 1)
        {
            node_by_effect.insert(effect.id, node_id.to_owned());
        }
    }
    for effect in &mut effects {
        effect.node_id = node_by_effect.get(&effect.id).cloned();
    }
    Ok(effects)
}

/// VM v1 receipts created before `EffectRequested` stored its instruction pointer contain the
/// evaluated request, while the frozen module still contains expressions. Exact equality therefore
/// cannot recover an action such as `console.run({ command: { $concat: [...] } })` after the VM has
/// resolved it to a string. Match the stable call-site identity instead, and let the caller's
/// single-candidate rule reject genuinely ambiguous workflows.
fn legacy_effect_call_site_matches(
    compiled: &runinator_models::workflow_vm::WorkflowEffectRequest,
    executed: &runinator_models::workflow_vm::WorkflowEffectRequest,
) -> bool {
    use runinator_models::workflow_vm::WorkflowEffectRequest;

    if compiled == executed {
        return true;
    }

    match (compiled, executed) {
        (
            WorkflowEffectRequest::Action {
                provider: compiled_provider,
                function: compiled_function,
                ..
            },
            WorkflowEffectRequest::Action {
                provider: executed_provider,
                function: executed_function,
                ..
            },
        ) => compiled_provider == executed_provider && compiled_function == executed_function,
        (WorkflowEffectRequest::Timer { .. }, WorkflowEffectRequest::Timer { .. })
        | (WorkflowEffectRequest::TimerDelay { .. }, WorkflowEffectRequest::TimerDelay { .. })
        | (WorkflowEffectRequest::Approval { .. }, WorkflowEffectRequest::Approval { .. })
        | (WorkflowEffectRequest::Input { .. }, WorkflowEffectRequest::Input { .. }) => true,
        (
            WorkflowEffectRequest::Gate {
                kind: compiled_kind,
                label: compiled_label,
                ..
            },
            WorkflowEffectRequest::Gate {
                kind: executed_kind,
                label: executed_label,
                ..
            },
        ) => compiled_kind == executed_kind && compiled_label == executed_label,
        (
            WorkflowEffectRequest::Signal {
                key: compiled_key, ..
            },
            WorkflowEffectRequest::Signal {
                key: executed_key, ..
            },
        ) => compiled_key == executed_key,
        (
            WorkflowEffectRequest::EventWait {
                event_type: compiled_type,
                ..
            },
            WorkflowEffectRequest::EventWait {
                event_type: executed_type,
                ..
            },
        ) => compiled_type == executed_type,
        (
            WorkflowEffectRequest::ChildRun {
                workflow_id: compiled_id,
                workflow_name: compiled_name,
                ..
            },
            WorkflowEffectRequest::ChildRun {
                workflow_id: executed_id,
                workflow_name: executed_name,
                ..
            },
        ) => compiled_id == executed_id && compiled_name == executed_name,
        (
            WorkflowEffectRequest::AwaitRun {
                workflow: compiled_workflow,
                ..
            },
            WorkflowEffectRequest::AwaitRun {
                workflow: executed_workflow,
                ..
            },
        ) => compiled_workflow == executed_workflow,
        (
            WorkflowEffectRequest::MutexAcquire { key: compiled_key },
            WorkflowEffectRequest::MutexAcquire { key: executed_key },
        ) => compiled_key == executed_key,
        (
            WorkflowEffectRequest::Coordination {
                kind: compiled_kind,
                ..
            },
            WorkflowEffectRequest::Coordination {
                kind: executed_kind,
                ..
            },
        ) => compiled_kind == executed_kind,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use runinator_models::{
        value::Value,
        workflow_vm::{
            WORKFLOW_JOURNAL_VERSION, WorkflowEffectRequest, WorkflowJournalEntry,
            WorkflowJournalRecord,
        },
        workflows::WorkflowRetry,
    };
    use uuid::Uuid;

    use super::{journal_transitions, legacy_effect_call_site_matches, node_transition_stats};

    fn action(input: Value) -> WorkflowEffectRequest {
        WorkflowEffectRequest::Action {
            provider: "console".into(),
            function: "run".into(),
            input,
            timeout_seconds: Some(300),
            retry: WorkflowRetry::default(),
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            idempotency_key: None,
            function_binding: None,
        }
    }

    #[test]
    fn legacy_projection_matches_a_resolved_action_input() {
        let compiled = action(runinator_models::json!({
            "command": { "$concat": ["echo ", "hello"] }
        }));
        let executed = action(runinator_models::json!({ "command": "echo hello" }));

        assert!(legacy_effect_call_site_matches(&compiled, &executed));
    }

    #[test]
    fn legacy_projection_does_not_cross_action_call_sites() {
        let compiled = action(runinator_models::json!({ "command": "echo hello" }));
        let mut executed = action(runinator_models::json!({ "command": "echo hello" }));
        let WorkflowEffectRequest::Action { provider, .. } = &mut executed else {
            unreachable!();
        };
        *provider = "git".into();

        assert!(!legacy_effect_call_site_matches(&compiled, &executed));
    }

    fn entered_node(
        id: u128,
        workflow_run_id: Uuid,
        continuation_id: Uuid,
        sequence: u64,
        node_id: &str,
        created_at: i64,
    ) -> WorkflowJournalRecord {
        WorkflowJournalRecord {
            version: WORKFLOW_JOURNAL_VERSION,
            id: Uuid::from_u128(id),
            workflow_run_id,
            sequence,
            continuation_id: Some(continuation_id),
            effect_id: None,
            entry: WorkflowJournalEntry::NodeEntered {
                continuation_id,
                node_id: node_id.to_owned(),
            },
            created_at,
        }
    }

    #[test]
    fn transition_projection_keeps_parallel_continuations_separate() {
        let workflow_run_id = Uuid::from_u128(100);
        let primary = Uuid::from_u128(101);
        let branch = Uuid::from_u128(102);
        let records = vec![
            entered_node(1, workflow_run_id, primary, 1, "start", 10),
            entered_node(2, workflow_run_id, branch, 2, "side", 11),
            entered_node(3, workflow_run_id, primary, 3, "finish", 12),
        ];

        let transitions = journal_transitions(records.clone());
        assert_eq!(transitions.len(), 3);
        assert_eq!(transitions[0].from_node, None);
        assert_eq!(transitions[1].from_node, None);
        assert_eq!(transitions[2].from_node.as_deref(), Some("start"));
        assert_eq!(transitions[2].to_node, "finish");

        let stats = node_transition_stats(records, "start");
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].to_node, "finish");
        assert_eq!(stats[0].count, 1);
        assert_eq!(stats[0].last_at.timestamp(), 12);
    }
}

pub async fn get_effect<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(record)) => {
            if let Err(reply) = authorize_run(db.as_ref(), &ctx, record.workflow_run_id).await {
                return reply;
            }
            (StatusCode::OK, Json(ApiResponse::WorkflowEffect(record)))
        }
        Ok(None) => not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_effect_output<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, effect.workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_effect_output(effect_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowEffectOutput(records)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// Route operator input to the desktop worker currently holding an interactive provider effect.
/// The payload is intentionally ephemeral; terminal output remains durable, but replaying input
/// after a worker reconnect would be unsafe.
pub async fn control_effect_terminal<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
    ValidatedJson(control): ValidatedJson<ProviderTerminalControl>,
) -> (StatusCode, Json<ApiResponse>) {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(effect.workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    let interactive_action = matches!(
        &effect.request,
        runinator_models::workflow_vm::WorkflowEffectRequest::Action {
            provider,
            function,
            input,
            ..
        } if ((provider == "console" && function == "run")
            || (provider == "ai-command" && function == "claude_code"))
            && input.get("interactive").and_then(Value::as_bool) == Some(true)
    );
    if !interactive_action {
        return runinator_ws_core::responses::bad_request(
            "terminal control is only available for interactive provider effects",
        );
    }
    if !matches!(
        effect.status,
        WorkflowEffectStatus::Running | WorkflowEffectStatus::InputRequired
    ) {
        return runinator_ws_core::responses::bad_request(
            "terminal control requires an active effect",
        );
    }
    let Some(replica_id) = effect.current_executor_replica_id else {
        return runinator_ws_core::responses::bad_request(
            "the terminal worker has not claimed this effect yet",
        );
    };
    let command = ControlCommand::for_terminal(effect.workflow_run_id, effect.id, control)
        .targeting_replica(replica_id);
    match broker.publish_control(command).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: format!("Terminal control sent to effect {effect_id}"),
            })),
        ),
        Err(error) => api_error(error.to_string()),
    }
}

/// Stream an artifact owned by one durable VM effect-output event. This deliberately addresses the
/// event rather than a legacy run_artifacts row: the journal output is the authoritative history.
pub async fn download_effect_artifact<
    T: AuthorizationStore + RuntimeStore + WorkflowVmStore + FileStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(files): Extension<Arc<WorkflowFiles<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path((effect_id, event_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return (StatusCode::NOT_FOUND, "workflow effect not found").into_response(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, effect.workflow_run_id).await {
        return reply.into_response();
    }
    let output = match db.fetch_workflow_effect_output(effect_id).await {
        Ok(events) => events.into_iter().find(|event| event.event_id == event_id),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    let Some(output) = output else {
        return (StatusCode::NOT_FOUND, "artifact output event not found").into_response();
    };
    let runinator_models::workflow_vm::WorkflowEffectOutput::Artifact { artifact } = output.output
    else {
        return (StatusCode::NOT_FOUND, "effect output is not an artifact").into_response();
    };
    let artifact = match artifact.decode::<runinator_models::runs::NewRunArtifact>() {
        Ok(artifact) => artifact,
        Err(_) => return (StatusCode::NOT_FOUND, "artifact metadata is invalid").into_response(),
    };
    let content = match files.open_artifact_uri(&artifact.uri).await {
        Ok(content) => content,
        Err(error) => return (StatusCode::NOT_FOUND, error.to_string()).into_response(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, artifact.mime_type)
        .header(header::CONTENT_LENGTH, content.size_bytes)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                artifact.name.replace('"', "_")
            ),
        )
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(
            content.body,
        )))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "response build failed").into_response()
        })
}

#[derive(Debug, Deserialize)]
pub struct SettleEffectRequest {
    pub status: WorkflowEffectStatus,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

impl Validate for SettleEffectRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !self.status.is_terminal() {
            return Err(ValidationError::new("status", "must be terminal"));
        }
        optional_text("message", self.message.as_deref(), LONG_TEXT_MAX)
    }
}

/// Resolve an approval/input/signal/gate/event wait by its durable effect identity. Provider
/// effects use the broker result path; accepting them here would bypass worker attempt ownership.
pub async fn settle_effect<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(effect_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<SettleEffectRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let effect = match db.fetch_workflow_effect(effect_id).await {
        Ok(Some(effect)) => effect,
        Ok(None) => return not_found(format!("workflow effect {effect_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_run_workflow(effect.workflow_run_id, Permission::Run)
        .await
    {
        return reply;
    }
    if matches!(
        effect.request,
        runinator_models::workflow_vm::WorkflowEffectRequest::Action { .. }
    ) {
        return runinator_ws_core::responses::bad_request(
            "provider effects can only be settled by their assigned worker",
        );
    }
    if !request.status.is_terminal() {
        return runinator_ws_core::responses::bad_request(
            "effect settlement status must be terminal",
        );
    }
    match db
        .settle_workflow_effect(
            effect_id,
            effect.attempt,
            request.status,
            request.output,
            request.message,
            chrono::Utc::now(),
        )
        .await
    {
        Ok(applied) => {
            if applied {
                let org_id = match db.fetch_workflow_run(effect.workflow_run_id).await {
                    Ok(Some(run)) => db
                        .fetch_workflow(run.workflow_id)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|workflow| workflow.org_id),
                    _ => None,
                };
                emit_workflow_run(&events, effect.workflow_run_id, org_id);
            }
            (
                StatusCode::OK,
                Json(ApiResponse::TaskResponse(TaskResponse {
                    success: applied,
                    message: if applied {
                        format!("Workflow effect {effect_id} settled")
                    } else {
                        format!("Workflow effect {effect_id} was already settled or stale")
                    },
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_journal<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_journal(workflow_run_id).await {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::WorkflowJournal(records))),
        Err(err) => api_error(err.to_string()),
    }
}

/// Project the graph edges a VM run took from the durable `NodeEntered` journal events. The
/// journal sequence is global to the run, while the predecessor must remain per continuation so
/// interleaved parallel branches never manufacture cross-branch edges.
fn journal_transitions(records: Vec<WorkflowJournalRecord>) -> Vec<NodeTransition> {
    let mut previous_node_by_continuation = HashMap::new();
    let mut transitions = Vec::new();

    for record in records {
        let WorkflowJournalEntry::NodeEntered {
            continuation_id,
            node_id,
        } = record.entry
        else {
            continue;
        };
        let Some(at) = chrono::DateTime::from_timestamp(record.created_at, 0) else {
            continue;
        };
        let from_node = previous_node_by_continuation.insert(continuation_id, node_id.clone());
        transitions.push(NodeTransition {
            from_node,
            to_node: node_id,
            reason: None,
            // The immutable journal id is the durable identity of this transition. Keep the
            // historical wire field name until a versioned client contract can rename it.
            node_run_id: record.id,
            at,
        });
    }

    transitions
}

fn node_transition_stats(
    records: Vec<WorkflowJournalRecord>,
    node_id: &str,
) -> Vec<NodeTransitionStat> {
    let mut stats = BTreeMap::<String, NodeTransitionStat>::new();

    for transition in journal_transitions(records) {
        if transition.from_node.as_deref() != Some(node_id) {
            continue;
        }
        let entry = stats
            .entry(transition.to_node.clone())
            .or_insert_with(|| NodeTransitionStat {
                from_node: node_id.to_owned(),
                to_node: transition.to_node.clone(),
                count: 0,
                last_reason: None,
                last_at: transition.at,
            });
        entry.count += 1;
        if transition.at >= entry.last_at {
            entry.last_at = transition.at;
            entry.last_reason = transition.reason;
        }
    }

    stats.into_values().collect()
}

pub async fn list_run_transitions<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    match db.fetch_workflow_journal(workflow_run_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::NodeTransitions(journal_transitions(records))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_node_transitions<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((workflow_id, node_id)): Path<(Uuid, String)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await
    {
        return reply;
    }
    match db.fetch_workflow_journals_for_workflow(workflow_id).await {
        Ok(records) => (
            StatusCode::OK,
            Json(ApiResponse::NodeTransitionStats(node_transition_stats(
                records, &node_id,
            ))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_cursors<T: AuthorizationStore + RuntimeStore + WorkflowVmStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = authorize_run(db.as_ref(), &ctx, workflow_run_id).await {
        return reply;
    }
    let module = match db.fetch_workflow_module(workflow_run_id).await {
        Ok(Some(module)) => module,
        Ok(None) => {
            return (
                StatusCode::OK,
                Json(ApiResponse::WorkflowVmCursors(Vec::new())),
            );
        }
        Err(err) => return api_error(err.to_string()),
    };
    match db.fetch_workflow_continuations(workflow_run_id).await {
        Ok(continuations) => {
            let cursors = continuations
                .into_iter()
                .map(|continuation| {
                    // A suspended continuation has already advanced past the yielding opcode; the
                    // operator-facing cursor still belongs to the node that produced the effect.
                    let instruction_pointer = if continuation.awaiting_effect_id.is_some() {
                        continuation.instruction_pointer.saturating_sub(1)
                    } else {
                        continuation.instruction_pointer
                    };
                    let location = module.graph_location(instruction_pointer);
                    let debug = continuation
                        .frames
                        .iter()
                        .rev()
                        .find_map(|frame| match frame {
                            runinator_models::workflow_vm::WorkflowFrame::Debug(debug) => {
                                Some(debug)
                            }
                            _ => None,
                        });
                    WorkflowVmCursor {
                        continuation_id: continuation.id,
                        instruction_pointer,
                        node_id: location.map(|entry| entry.node_id.clone()),
                        edge_label: location.and_then(|entry| entry.edge_label.clone()),
                        status: continuation.status,
                        stop_reason: debug.and_then(|frame| {
                            if frame.pending_failure.is_some() {
                                Some("failure".to_string())
                            } else if frame.paused {
                                Some("breakpoint".to_string())
                            } else {
                                None
                            }
                        }),
                        run_to_node_id: debug.and_then(|frame| frame.run_to_node_id.clone()),
                        pending_failure: debug.and_then(|frame| frame.pending_failure.clone()),
                    }
                })
                .collect();
            (
                StatusCode::OK,
                Json(ApiResponse::WorkflowVmCursors(cursors)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub fn routes<T: AuthorizationStore + RuntimeStore + WorkflowVmStore + FileStore>(
    pool: Arc<T>,
) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/workflow_runs/{id}/continuations",
            get(list_continuations::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/effects",
            get(list_effects::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/journal",
            get(list_journal::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/transitions",
            get(list_run_transitions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/nodes/{node_id}/transitions",
            get(list_node_transitions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/cursors",
            get(list_cursors::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_continuations/{id}",
            get(get_continuation::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}",
            get(get_effect::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/output",
            get(list_effect_output::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/terminal",
            post(control_effect_terminal::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/output/{event_id}/artifact",
            get(download_effect_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_effects/{id}/settle",
            post(settle_effect::<T>).layer(Extension(pool)),
        )
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/workflow_runs/{id}/continuations",
        "Workflow VM",
        "List continuations",
        "Lists the durable branches of a VM-backed workflow run.",
        false,
        None,
        &[],
        200,
        "continuations",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/workflow_effects/{id}/terminal",
        "Workflow VM",
        "Control an interactive effect terminal",
        "Routes terminal input, resize, or EOF to the worker currently executing an interactive console effect.",
        false,
        None,
        &[],
        202,
        "terminal control accepted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/effects",
        "Workflow VM",
        "List effects",
        "Lists durable VM effects without reading node-run records.",
        false,
        None,
        &[],
        200,
        "effects",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/journal",
        "Workflow VM",
        "Read execution journal",
        "Returns the immutable VM execution history in sequence order.",
        false,
        None,
        &[],
        200,
        "journal",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/transitions",
        "Workflow VM",
        "Read run transition path",
        "Projects the graph edges a workflow run actually traversed from its immutable VM journal.",
        false,
        None,
        &[],
        200,
        "workflow run transitions",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflows/{id}/nodes/{node_id}/transitions",
        "Workflow VM",
        "Read node transition statistics",
        "Aggregates the outgoing graph edges observed from one workflow node across its workflow runs.",
        false,
        None,
        &[],
        200,
        "workflow node transition statistics",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/cursors",
        "Workflow VM",
        "Render graph cursors",
        "Projects continuation instruction pointers through the frozen module source map.",
        false,
        None,
        &[],
        200,
        "graph cursors",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_continuations/{id}",
        "Workflow VM",
        "Get continuation",
        "Returns one durable continuation by its execution identity.",
        false,
        None,
        &[],
        200,
        "continuation",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_effects/{id}",
        "Workflow VM",
        "Get effect",
        "Returns one durable VM effect by its identity.",
        false,
        None,
        &[],
        200,
        "effect",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_effects/{id}/output",
        "Workflow VM",
        "List effect output",
        "Returns the durable output events recorded for one workflow VM effect.",
        false,
        None,
        &[],
        200,
        "effect output events",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_effects/{id}/output/{event_id}/artifact",
        "Workflow VM",
        "Download effect artifact",
        "Streams an artifact recorded in a VM effect output event after authorizing access to its workflow run.",
        false,
        None,
        &[],
        200,
        "artifact bytes",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/workflow_effects/{id}/settle",
        "Workflow VM",
        "Settle effect",
        "Settles a non-provider workflow effect with a terminal status and optional output.",
        false,
        None,
        &[],
        200,
        "effect settlement result",
        Example::TaskResponse,
    ),
];
