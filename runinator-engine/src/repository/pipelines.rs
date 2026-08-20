use super::*;
use runinator_models::pipelines::{
    PIPELINE_GRAPH_VERSION, PipelineBundle, PipelineGraph, PipelineJoin, PipelineLink,
    PipelineMember, PipelineRun, PipelineRunDetail, PipelineRunEdgeState, PipelineRunJoinState,
    PipelineSpec, PipelineTrigger,
};
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};
use uuid::Uuid;

pub async fn upsert_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline: &Pipeline,
) -> Result<Pipeline, SendableError> {
    validate_pipeline(pipeline)?;
    db.upsert_pipeline(pipeline).await
}

fn invalid_pipeline(message: impl Into<String>) -> SendableError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

fn validate_mapping(value: &Value) -> Result<(), SendableError> {
    runinator_workflows::validate_expression(value)
        .map_err(|error| invalid_pipeline(error.to_string()))?;
    fn walk(value: &Value) -> Result<(), SendableError> {
        match value {
            Value::Array(values) => values.iter().try_for_each(walk),
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_object)
                    && let Some(root) = reference.get("node").and_then(Value::as_str)
                    && !matches!(root, "source" | "members")
                {
                    return Err(invalid_pipeline(format!(
                        "unsupported pipeline mapping root '{root}'"
                    )));
                }
                if let Some(call) = object.get("$call").and_then(Value::as_str) {
                    let leaf = call.rsplit('.').next().unwrap_or(call);
                    if !runinator_workflows::PureIntrinsics::contains(leaf)
                        && !runinator_workflows::is_higher_order(leaf)
                    {
                        return Err(invalid_pipeline(format!(
                            "pipeline mapping call '{call}' is not pure"
                        )));
                    }
                }
                object.values().try_for_each(walk)
            }
            _ => Ok(()),
        }
    }
    walk(value)
}

fn validate_pipeline(pipeline: &Pipeline) -> Result<(), SendableError> {
    if !pipeline.graph.is_current() {
        return Err(invalid_pipeline(
            "pipeline graph requires source pack reimport",
        ));
    }
    if pipeline.concurrency.max_concurrent_runs < 0 {
        return Err(invalid_pipeline("max_concurrent_runs cannot be negative"));
    }
    let mut members = std::collections::HashSet::new();
    for member in &pipeline.graph.members {
        if member.key.trim().is_empty() || !members.insert(member.key.as_str()) {
            return Err(invalid_pipeline(format!(
                "duplicate or empty pipeline member key '{}'",
                member.key
            )));
        }
    }
    let mut links = std::collections::HashSet::new();
    let mut link_ids = std::collections::HashSet::new();
    let mut inbound: std::collections::HashMap<&str, Vec<&PipelineLink>> =
        std::collections::HashMap::new();
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for link in &pipeline.graph.links {
        if !members.contains(link.from.as_str()) || !members.contains(link.to.as_str()) {
            return Err(invalid_pipeline(format!(
                "pipeline link {} -> {} names an unknown member",
                link.from, link.to
            )));
        }
        if link.from == link.to
            || !links.insert((link.from.as_str(), link.to.as_str()))
            || !link_ids.insert(link.id)
        {
            return Err(invalid_pipeline(format!(
                "duplicate, self, or reused-id pipeline link {} -> {}",
                link.from, link.to
            )));
        }
        validate_mapping(&link.parameters)?;
        if link.enabled {
            inbound.entry(&link.to).or_default().push(link);
            adjacency.entry(&link.from).or_default().push(&link.to);
        }
    }
    fn cyclic<'a>(
        node: &'a str,
        adjacency: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut std::collections::HashSet<&'a str>,
        done: &mut std::collections::HashSet<&'a str>,
    ) -> bool {
        if done.contains(node) {
            return false;
        }
        if !visiting.insert(node) {
            return true;
        }
        if adjacency.get(node).is_some_and(|next| {
            next.iter()
                .any(|next| cyclic(next, adjacency, visiting, done))
        }) {
            return true;
        }
        visiting.remove(node);
        done.insert(node);
        false
    }
    let mut visiting = std::collections::HashSet::new();
    let mut done = std::collections::HashSet::new();
    if members
        .iter()
        .any(|member| cyclic(member, &adjacency, &mut visiting, &mut done))
    {
        return Err(invalid_pipeline("pipeline graph contains a cycle"));
    }
    for (target, incoming) in &inbound {
        if incoming.len() > 1 && !pipeline.graph.joins.contains_key(*target) {
            return Err(invalid_pipeline(format!(
                "member '{target}' has multiple inbound links but no join"
            )));
        }
    }
    for (key, join) in &pipeline.graph.joins {
        if key != &join.target || !members.contains(key.as_str()) {
            return Err(invalid_pipeline(format!(
                "join '{key}' has an invalid target"
            )));
        }
        let incoming = inbound
            .get(key.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if incoming.len() < 2 {
            return Err(invalid_pipeline(format!(
                "join '{key}' requires at least two enabled inputs"
            )));
        }
        if join.mode == runinator_models::pipelines::PipelineJoinMode::FirstSuccess
            && incoming
                .iter()
                .any(|link| link.on != runinator_models::pipelines::PipelineLinkSelector::Success)
        {
            return Err(invalid_pipeline(format!(
                "first_success join '{key}' requires success selectors"
            )));
        }
        validate_mapping(&join.parameters)?;
    }
    Ok(())
}

/// import a compiled pipeline bundle from a pack. for each pipeline: resolve member workflow names to
/// ids, upsert the pipeline (reusing an existing id with the same name + org so re-import updates in
/// place), and atomically replace its first-class graph. pack-managed pipelines carry
/// `metadata.managed_by = "rexrap"`; only pipeline start triggers are materialized separately.
pub async fn import_pipeline_bundle_with<T: DatabaseImpl>(
    db: &T,
    bundle: &PipelineBundle,
    import_org: Option<Uuid>,
) -> Result<Vec<Pipeline>, SendableError> {
    let existing = db.fetch_pipelines().await?;
    let mut imported = Vec::with_capacity(bundle.pipelines.len());
    for spec in &bundle.pipelines {
        imported.push(import_pipeline_spec(db, spec, import_org, &existing).await?);
    }
    Ok(imported)
}

async fn import_pipeline_spec<T: DatabaseImpl>(
    db: &T,
    spec: &PipelineSpec,
    import_org: Option<Uuid>,
    existing: &[Pipeline],
) -> Result<Pipeline, SendableError> {
    // resolve each member workflow name to its id; an unknown member fails the import loudly.
    let mut graph_members = Vec::with_capacity(spec.members.len());
    for member in &spec.members {
        let id = db
            .fetch_workflow_by_name(member.name.clone())
            .await?
            .and_then(|workflow| workflow.id)
            .ok_or_else(|| {
                crate::errors::IMPORT_UNKNOWN_PIPELINE_MEMBER.error(member.name.as_str())
            })?;
        graph_members.push(PipelineMember {
            key: member.name.clone(),
            workflow_id: id,
            failure_mode: member
                .failure_mode
                .unwrap_or(spec.defaults.default_failure_mode),
        });
    }
    // reuse the id of an existing pipeline with the same name and org so re-import updates in place.
    let prior = existing
        .iter()
        .find(|p| p.name == spec.name && p.org_id == import_org);
    let prior_id = prior.and_then(|p| p.id);
    let pipeline = Pipeline {
        id: prior_id,
        name: spec.name.clone(),
        description: spec.description.clone(),
        org_id: import_org,
        graph: PipelineGraph {
            version: PIPELINE_GRAPH_VERSION,
            members: graph_members,
            links: spec
                .links
                .iter()
                .map(|link| PipelineLink {
                    id: prior
                        .and_then(|pipeline| {
                            pipeline
                                .graph
                                .links
                                .iter()
                                .find(|prior| prior.from == link.from && prior.to == link.to)
                        })
                        .map(|prior| prior.id)
                        .unwrap_or_else(Uuid::new_v4),
                    from: link.from.clone(),
                    to: link.to.clone(),
                    on: link.on,
                    enabled: link.enabled,
                    parameters: link.parameters.clone(),
                })
                .collect(),
            joins: spec
                .joins
                .iter()
                .map(|join| {
                    (
                        join.target.clone(),
                        PipelineJoin {
                            target: join.target.clone(),
                            mode: join.mode,
                            parameters: join.parameters.clone(),
                        },
                    )
                })
                .collect(),
        },
        concurrency: spec.concurrency,
        defaults: spec.defaults.clone(),
        metadata: runinator_models::json!({ "managed_by": "rexrap", "requires_reimport": false }),
        created_at: None,
        updated_at: None,
    };
    validate_pipeline(&pipeline)?;
    let saved = db.upsert_pipeline(&pipeline).await?;
    let pipeline_id = saved
        .id
        .ok_or_else(|| crate::errors::IMPORT_UNKNOWN_PIPELINE_MEMBER.error(spec.name.as_str()))?;
    materialize_pipeline_triggers(db, spec, pipeline_id).await?;
    Ok(saved)
}

// realize a pipeline's header triggers as managed `pipeline_triggers`. reconciles idempotently: drop
// this pipeline's prior managed triggers, then insert the current specs. manually-created pipeline
// triggers (no `managed_by == "rexrap"`) are left untouched.
async fn materialize_pipeline_triggers<T: DatabaseImpl>(
    db: &T,
    spec: &PipelineSpec,
    pipeline_id: Uuid,
) -> Result<(), SendableError> {
    for existing in db.fetch_pipeline_triggers(pipeline_id).await? {
        let managed = existing
            .metadata
            .pointer("/managed_by")
            .and_then(Value::as_str)
            == Some("rexrap");
        if let (true, Some(trigger_id)) = (managed, existing.id) {
            db.delete_pipeline_trigger(trigger_id).await?;
        }
    }
    for spec_trigger in &spec.triggers {
        let trigger = PipelineTrigger {
            id: None,
            pipeline_id,
            kind: spec_trigger.kind.clone(),
            enabled: spec_trigger.enabled,
            configuration: spec_trigger.configuration.clone(),
            next_execution: None,
            blackout_start: None,
            blackout_end: None,
            metadata: runinator_models::json!({ "managed_by": "rexrap" }),
            created_at: None,
            updated_at: None,
        };
        db.upsert_pipeline_trigger(&trigger).await?;
    }
    Ok(())
}

pub async fn fetch_pipelines<T: DatabaseImpl>(db: &T) -> Result<Vec<Pipeline>, SendableError> {
    db.fetch_pipelines().await
}

pub async fn fetch_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Option<Pipeline>, SendableError> {
    db.fetch_pipeline(pipeline_id).await
}

pub async fn delete_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline(pipeline_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline deleted".into(),
    })
}

pub async fn set_pipeline_org<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    org_id: Option<Uuid>,
) -> Result<(), SendableError> {
    db.set_pipeline_org(pipeline_id, org_id).await
}

// --- pipeline triggers ---

pub async fn upsert_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger: &PipelineTrigger,
) -> Result<PipelineTrigger, SendableError> {
    db.upsert_pipeline_trigger(trigger).await
}

pub async fn fetch_pipeline_triggers<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Vec<PipelineTrigger>, SendableError> {
    db.fetch_pipeline_triggers(pipeline_id).await
}

pub async fn fetch_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<Option<PipelineTrigger>, SendableError> {
    db.fetch_pipeline_trigger(trigger_id).await
}

pub async fn delete_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline_trigger(trigger_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline trigger deleted".into(),
    })
}

// --- pipeline runs ---

/// start a manual pipeline run for a pipeline id (creates the run and its entry members).
pub async fn create_manual_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    parameters: Value,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let pipeline = db
        .fetch_pipeline(pipeline_id)
        .await?
        .ok_or_else(|| runinator_reducer::errors::PIPELINE_NOT_FOUND.error(pipeline_id))?;
    let provenance = WorkflowRunProvenance {
        source_kind: Some(TriggerSourceKind::Manual),
        actor_type: Some(TriggerActorType::User),
        actor_replica_id,
        actor_display_name,
        request_host: None,
        request_ip: None,
        metadata: Value::Object(Default::default()),
    };
    runinator_reducer::create_and_start_pipeline_run(db, &pipeline, parameters, provenance).await
}

/// start a pipeline run from a manual/cron pipeline trigger id.
pub async fn create_pipeline_run_for_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
    parameters: Value,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let trigger = db
        .fetch_pipeline_trigger(trigger_id)
        .await?
        .ok_or_else(|| runinator_reducer::errors::PIPELINE_TRIGGER_NOT_FOUND.error(trigger_id))?;
    let effective =
        if parameters.is_null() || matches!(&parameters, Value::Object(map) if map.is_empty()) {
            trigger
                .configuration
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()))
        } else {
            parameters
        };
    create_manual_pipeline_run(
        db,
        trigger.pipeline_id,
        effective,
        actor_replica_id,
        actor_display_name,
    )
    .await
}

/// fire due cron pipeline triggers and start each created run's entry members. mirrors the workflow
/// trigger claim wrapper.
pub async fn claim_due_pipeline_trigger_firings<T: DatabaseImpl>(
    db: &T,
    scheduler_id: String,
    limit: i64,
) -> Result<Vec<PipelineRun>, SendableError> {
    let runs = db
        .claim_due_pipeline_trigger_firings(scheduler_id, Utc::now(), limit)
        .await?;
    let mut admitted = Vec::with_capacity(runs.len());
    for run in runs {
        let outcome = runinator_reducer::start_pipeline_run(db, &run).await?;
        if outcome != runinator_reducer::PipelineStartOutcome::Skipped {
            admitted.push(run);
        }
    }
    Ok(admitted)
}

pub async fn fetch_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<Option<PipelineRun>, SendableError> {
    db.fetch_pipeline_run(pipeline_run_id).await
}

pub async fn fetch_recent_pipeline_runs<T: DatabaseImpl>(
    db: &T,
    limit: i64,
) -> Result<Vec<PipelineRun>, SendableError> {
    db.fetch_recent_pipeline_runs(limit).await
}

/// fetch a pipeline run together with the member workflow runs it started.
pub async fn fetch_pipeline_run_detail<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<Option<PipelineRunDetail>, SendableError> {
    let Some(run) = db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Ok(None);
    };
    let members = db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?;
    let attempts = db.fetch_pipeline_member_attempts(pipeline_run_id).await?;
    let latest = |key: &str| {
        attempts
            .iter()
            .filter(|attempt| attempt.member_key == key)
            .max_by_key(|attempt| attempt.attempt)
    };
    let edges = run.pipeline_snapshot.as_ref().map(|pipeline| pipeline.graph.links.iter().map(|link| {
        let source = latest(&link.from);
        let target = latest(&link.to);
        let stopped = source.is_some_and(|attempt| {
            matches!(attempt.status, runinator_models::pipelines::PipelineMemberAttemptStatus::Failed | runinator_models::pipelines::PipelineMemberAttemptStatus::TimedOut)
                && pipeline.graph.members.iter().find(|member| member.key == link.from)
                    .is_some_and(|member| member.failure_mode == runinator_models::pipelines::PipelineMemberFailureMode::Stop)
        });
        let state = match source {
            None => "pending",
            Some(source) if !source.status.is_terminal() => "pending",
            Some(_) if stopped => "skipped",
            Some(source) if match link.on {
                runinator_models::pipelines::PipelineLinkSelector::Success => source.status == runinator_models::pipelines::PipelineMemberAttemptStatus::Succeeded,
                runinator_models::pipelines::PipelineLinkSelector::Failure => matches!(source.status, runinator_models::pipelines::PipelineMemberAttemptStatus::Failed | runinator_models::pipelines::PipelineMemberAttemptStatus::TimedOut),
                runinator_models::pipelines::PipelineLinkSelector::Complete => source.status != runinator_models::pipelines::PipelineMemberAttemptStatus::Skipped,
            } => if target.is_some_and(|target| !target.status.is_terminal()) { "active" } else { "satisfied" },
            Some(_) => "skipped",
        };
        PipelineRunEdgeState { link_id: link.id, state: state.into() }
    }).collect()).unwrap_or_default();
    let joins = run.pipeline_snapshot.as_ref().map(|pipeline| pipeline.graph.joins.values().map(|join| {
        let inbound = pipeline.graph.links.iter().filter(|link| link.enabled && link.to == join.target).collect::<Vec<_>>();
        let satisfied = inbound.iter().filter(|link| latest(&link.from).is_some_and(|attempt| {
            let stopped = matches!(attempt.status, runinator_models::pipelines::PipelineMemberAttemptStatus::Failed | runinator_models::pipelines::PipelineMemberAttemptStatus::TimedOut)
                && pipeline.graph.members.iter().find(|member| member.key == link.from)
                    .is_some_and(|member| member.failure_mode == runinator_models::pipelines::PipelineMemberFailureMode::Stop);
            !stopped && match link.on {
            runinator_models::pipelines::PipelineLinkSelector::Success => attempt.status == runinator_models::pipelines::PipelineMemberAttemptStatus::Succeeded,
            runinator_models::pipelines::PipelineLinkSelector::Failure => matches!(attempt.status, runinator_models::pipelines::PipelineMemberAttemptStatus::Failed | runinator_models::pipelines::PipelineMemberAttemptStatus::TimedOut),
            runinator_models::pipelines::PipelineLinkSelector::Complete => attempt.status.is_terminal() && attempt.status != runinator_models::pipelines::PipelineMemberAttemptStatus::Skipped,
        }})).count();
        let terminal = inbound.iter().filter(|link| latest(&link.from).is_some_and(|attempt| attempt.status.is_terminal())).count();
        let target = latest(&join.target);
        let ready = match join.mode { runinator_models::pipelines::PipelineJoinMode::All => satisfied == inbound.len(), _ => satisfied > 0 };
        let state = if target.is_some_and(|attempt| !attempt.status.is_terminal()) { "active" }
            else if target.is_some() || ready { "satisfied" }
            else if terminal == inbound.len() { "skipped" } else { "pending" };
        PipelineRunJoinState { target: join.target.clone(), mode: join.mode, state: state.into(), satisfied_inputs: satisfied, total_inputs: inbound.len() }
    }).collect()).unwrap_or_default();
    Ok(Some(PipelineRunDetail {
        run,
        members,
        attempts,
        edges,
        joins,
    }))
}

pub async fn fetch_pipeline_runs_for_pipeline<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Vec<PipelineRun>, SendableError> {
    db.fetch_pipeline_runs_for_pipeline(pipeline_id).await
}

pub async fn delete_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_pipeline_run(pipeline_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline run deleted".into(),
    })
}

/// cancel a pipeline run and every active member workflow run it owns.
pub async fn cancel_pipeline_run<T: DatabaseImpl>(
    db: &T,
    broker: &dyn runinator_broker_core::Broker,
    pipeline_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    for member in db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?
    {
        if member.status.is_active() {
            super::cancel_workflow_run(db, broker, member.id).await?;
        }
    }
    for attempt in db.fetch_pipeline_member_attempts(pipeline_run_id).await? {
        if !attempt.status.is_terminal() {
            db.update_pipeline_member_attempt(
                attempt.id,
                runinator_models::pipelines::PipelineMemberAttemptStatus::Canceled,
                attempt.result,
                Some("Pipeline run canceled".into()),
            )
            .await?;
        }
    }
    db.update_pipeline_run_status(
        pipeline_run_id,
        WorkflowStatus::Canceled,
        None,
        Some("Pipeline run canceled".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Pipeline run canceled".into(),
    })
}

pub async fn pause_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let Some(run) = db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Err(crate::errors::PIPELINE_PAUSE_NOT_FOUND.error(pipeline_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Pipeline run {pipeline_run_id} is already terminal"),
        });
    }
    for member in db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?
    {
        if member.status.is_active() {
            super::pause_workflow_run(db, member.id).await?;
        }
    }
    let mut state = run.state;
    if let Some(object) = state.as_object_mut() {
        object.insert(
            "control".into(),
            runinator_models::json!({ "pause_requested": true }),
        );
    }
    db.update_pipeline_run_status(
        pipeline_run_id,
        WorkflowStatus::Paused,
        Some(state),
        Some("Pipeline run paused".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Pipeline run {pipeline_run_id} paused"),
    })
}

pub async fn resume_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let Some(run) = db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Err(crate::errors::PIPELINE_RESUME_NOT_FOUND.error(pipeline_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Pipeline run {pipeline_run_id} is already terminal"),
        });
    }
    for member in db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?
    {
        if member.status.is_active() {
            super::resume_workflow_run(db, member.id).await?;
        }
    }
    runinator_reducer::resume_pipeline_run(db, pipeline_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Pipeline run {pipeline_run_id} resumed"),
    })
}

/// resolve a pipeline run's pending inquiry (a member with `Inquire` failure mode paused it).
/// `continue_pipeline` fires the failed member's onward pipeline links and resumes; `false` aborts
/// (settles the pipeline run `failed` now).
pub async fn resolve_pipeline_run_inquiry<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
    continue_pipeline: bool,
    resolved_by: Option<String>,
    message: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let decision = if continue_pipeline {
        runinator_reducer::PipelineInquiryDecision::Continue
    } else {
        runinator_reducer::PipelineInquiryDecision::Abort
    };
    runinator_reducer::resolve_pipeline_run_inquiry(
        db,
        pipeline_run_id,
        decision,
        resolved_by,
        message,
    )
    .await
}

pub async fn retry_pipeline_run_member<T: DatabaseImpl>(
    db: &T,
    pipeline_run_id: Uuid,
    member_key: String,
    parameter_override: Value,
) -> Result<runinator_models::pipelines::PipelineMemberAttempt, SendableError> {
    runinator_reducer::retry_pipeline_member(db, pipeline_run_id, &member_key, parameter_override)
        .await
}
