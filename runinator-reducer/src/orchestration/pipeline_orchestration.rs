use std::collections::{HashMap, HashSet};

use super::*;
use runinator_models::pipelines::{Pipeline, PipelineRun, PipelineTrigger};
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};

// max hops in a chain of pipeline-to-pipeline triggers before we stop, bounding accidental cycles.
const MAX_PIPELINE_CHAIN_DEPTH: i64 = 32;

/// create a pipeline run for `pipeline` and start its entry members. used by manual/api starts and by
/// chained-to-pipeline firing. returns the created run (already advanced to `running`, or settled
/// `failed` when the pipeline has no entry members).
pub async fn create_and_start_pipeline_run<T: DatabaseImpl>(
    db: &T,
    pipeline: &Pipeline,
    parameters: Value,
    provenance: WorkflowRunProvenance,
) -> Result<PipelineRun, SendableError> {
    let Some(pipeline_id) = pipeline.id else {
        return Err(crate::errors::PIPELINE_NOT_FOUND.error("pipeline is missing an id"));
    };
    let state = runinator_models::json!({ "trigger": provenance.metadata.clone() });
    let run = db
        .create_pipeline_run(pipeline_id, pipeline.clone(), parameters, state, provenance)
        .await?;
    start_pipeline_run(db, &run).await?;
    Ok(run)
}

/// start a pipeline run's entry members. the pipeline_runs row already exists (queued). computes the
/// entry members (members with no in-pipeline inbound link), starts each as a tagged workflow run, and
/// flips the pipeline run to `running`; settles `failed` when there are no entry members to start.
pub async fn start_pipeline_run<T: DatabaseImpl>(
    db: &T,
    run: &PipelineRun,
) -> Result<(), SendableError> {
    let pipeline = match run.pipeline_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => db
            .fetch_pipeline(run.pipeline_id)
            .await?
            .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(run.pipeline_id))?,
    };
    let entry = pipeline_entry_members(db, &pipeline).await?;
    if entry.is_empty() {
        db.update_pipeline_run_status(
            run.id,
            WorkflowStatus::Failed,
            None,
            Some("Pipeline has no entry members to start".into()),
        )
        .await?;
        return Err(crate::errors::PIPELINE_NO_ENTRY_MEMBERS.error(run.pipeline_id));
    }
    db.update_pipeline_run_status(run.id, WorkflowStatus::Running, None, None)
        .await?;
    for workflow_id in entry {
        start_member_run(db, run, workflow_id).await?;
    }
    Ok(())
}

/// resolve the entry member workflow ids of a pipeline: members that are never the target of an
/// in-pipeline `chained` link (a chained trigger carrying this pipeline's `configuration.pipeline_id`).
async fn pipeline_entry_members<T: DatabaseImpl>(
    db: &T,
    pipeline: &Pipeline,
) -> Result<Vec<Uuid>, SendableError> {
    let Some(pipeline_id) = pipeline.id else {
        return Ok(Vec::new());
    };
    let pipeline_key = pipeline_id.to_string();
    // build member id <-> name maps.
    let mut name_to_id: HashMap<String, Uuid> = HashMap::new();
    let mut member_ids: Vec<Uuid> = Vec::new();
    for id in &pipeline.workflow_ids {
        if let Some(workflow) = db.fetch_workflow(*id).await? {
            if let Some(wid) = workflow.id {
                name_to_id.insert(workflow.name.clone(), wid);
                member_ids.push(wid);
            }
        }
    }
    // any member that is the target of an in-pipeline link is downstream, not an entry point.
    let mut downstream: HashSet<Uuid> = HashSet::new();
    for member_id in &member_ids {
        for trigger in db.fetch_workflow_triggers(*member_id).await? {
            let belongs = trigger
                .configuration
                .pointer("/pipeline_id")
                .and_then(Value::as_str)
                == Some(pipeline_key.as_str());
            if !belongs {
                continue;
            }
            if let Some(target) = trigger
                .configuration
                .get("target_workflow")
                .and_then(Value::as_str)
                .and_then(|name| name_to_id.get(name))
            {
                downstream.insert(*target);
            }
        }
    }
    Ok(member_ids
        .into_iter()
        .filter(|id| !downstream.contains(id))
        .collect())
}

/// start a single member workflow run tagged with the owning pipeline run and enqueue its start node.
async fn start_member_run<T: DatabaseImpl>(
    db: &T,
    pipeline_run: &PipelineRun,
    workflow_id: Uuid,
) -> Result<(), SendableError> {
    let Some(snapshot) = db.fetch_workflow(workflow_id).await? else {
        // a missing member is skipped rather than failing the whole pipeline start.
        tracing::warn!(%workflow_id, "pipeline member workflow no longer exists; skipping");
        return Ok(());
    };
    let state = runinator_models::json!({ "control": { "pause_requested": false } });
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot.clone(),
            pipeline_run.parameters.clone(),
            state,
            None,
            WorkflowRunProvenance {
                source_kind: Some(TriggerSourceKind::Pipeline),
                actor_type: Some(TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("pipeline".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({ "pipeline_run_id": pipeline_run.id }),
            },
        )
        .await?;
    db.set_workflow_run_pipeline_run(run.id, pipeline_run.id)
        .await?;
    let (start, _) = runinator_workflows::parse_nodes(&snapshot)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let event = NewOrchestrationEvent::new(
        run.id,
        Some(start.clone()),
        "workflow_run_created",
        runinator_models::json!({ "workflow_id": workflow_id, "node_id": start }),
    );
    db.enqueue_ready_node(event, start, Utc::now()).await?;
    Ok(())
}

/// when a member workflow run reaches terminal, settle its owning pipeline run if the whole reachable
/// member graph is now terminal. no-op for runs not tagged with a pipeline run or already-settled runs.
pub(super) async fn maybe_settle_pipeline_run<T: DatabaseImpl>(
    db: &T,
    member_run: &WorkflowRun,
) -> Result<(), SendableError> {
    let Some(pipeline_run_id) = member_run.pipeline_run_id else {
        return Ok(());
    };
    let Some(pipeline_run) = db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Ok(());
    };
    if pipeline_run.status.is_terminal() {
        return Ok(());
    }
    let members = db
        .fetch_workflow_runs_for_pipeline_run(pipeline_run_id)
        .await?;
    // still running while any member is active (a newly-chained downstream member counts as active).
    if members.iter().any(|run| run.status.is_active()) {
        return Ok(());
    }
    let any_failed = members.iter().any(|run| {
        matches!(
            run.status,
            WorkflowStatus::Failed | WorkflowStatus::TimedOut
        )
    });
    let any_canceled = members
        .iter()
        .any(|run| run.status == WorkflowStatus::Canceled);
    let (status, message) = if any_failed {
        (
            WorkflowStatus::Failed,
            Some("A pipeline member failed".into()),
        )
    } else if any_canceled {
        (
            WorkflowStatus::Canceled,
            Some("A pipeline member was canceled".into()),
        )
    } else {
        (WorkflowStatus::Succeeded, None)
    };
    db.update_pipeline_run_status(pipeline_run_id, status, None, message)
        .await?;
    // a settled pipeline can itself be the source of a chained-to-pipeline trigger.
    let mut settled = pipeline_run;
    settled.status = status;
    maybe_start_chained_pipelines_from_pipeline(db, &settled).await?;
    Ok(())
}

/// start any pipelines chained to a terminal workflow run via an enabled `chained` pipeline trigger
/// whose `configuration.source_workflow` matches. deduped per (trigger, source run).
pub(super) async fn maybe_start_chained_pipelines<T: DatabaseImpl>(
    db: &T,
    source_run: &WorkflowRun,
) -> Result<(), SendableError> {
    if !source_run.status.is_terminal() {
        return Ok(());
    }
    // subflow/map children never fan out further chains.
    if source_run.state.get("subflow_parent").is_some()
        || source_run.state.get("map_child").is_some()
    {
        return Ok(());
    }
    let source_name = workflow_run_name(db, source_run).await?;
    let triggers = db.fetch_enabled_chained_pipeline_triggers().await?;
    for trigger in triggers {
        let matches_source = trigger
            .configuration
            .get("source_workflow")
            .and_then(Value::as_str)
            == Some(source_name.as_str());
        if !matches_source {
            continue;
        }
        start_chained_pipeline(db, &trigger, source_run.status, source_run.id, 0).await?;
    }
    Ok(())
}

/// start any pipelines chained to a terminal pipeline run via a `chained` pipeline trigger whose
/// `configuration.source_pipeline` matches. bounds cycles with a chain-depth guard.
async fn maybe_start_chained_pipelines_from_pipeline<T: DatabaseImpl>(
    db: &T,
    source_run: &PipelineRun,
) -> Result<(), SendableError> {
    if !source_run.status.is_terminal() {
        return Ok(());
    }
    let depth = source_run
        .trigger_metadata
        .get("pipeline_chain_depth")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if depth >= MAX_PIPELINE_CHAIN_DEPTH {
        tracing::warn!(pipeline_run_id = %source_run.id, depth, "pipeline chain depth limit reached");
        return Ok(());
    }
    let Some(source_pipeline) = db.fetch_pipeline(source_run.pipeline_id).await? else {
        return Ok(());
    };
    let triggers = db.fetch_enabled_chained_pipeline_triggers().await?;
    for trigger in triggers {
        let matches_source = trigger
            .configuration
            .get("source_pipeline")
            .and_then(Value::as_str)
            == Some(source_pipeline.name.as_str());
        if !matches_source {
            continue;
        }
        start_chained_pipeline(db, &trigger, source_run.status, source_run.id, depth + 1).await?;
    }
    Ok(())
}

/// shared chained-pipeline start: match the `on` selector, dedupe per (trigger, source run), then
/// create and start a pipeline run for the trigger's pipeline.
async fn start_chained_pipeline<T: DatabaseImpl>(
    db: &T,
    trigger: &PipelineTrigger,
    source_status: WorkflowStatus,
    source_run_id: Uuid,
    depth: i64,
) -> Result<(), SendableError> {
    if !pipeline_chain_status_matches(trigger, source_status) {
        return Ok(());
    }
    let Some(trigger_id) = trigger.id else {
        return Ok(());
    };
    if !db
        .try_record_pipeline_trigger_firing(trigger_id, source_run_id.to_string())
        .await?
    {
        return Ok(());
    }
    let Some(pipeline) = db.fetch_pipeline(trigger.pipeline_id).await? else {
        return Ok(());
    };
    let parameters = trigger
        .configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let provenance = WorkflowRunProvenance {
        source_kind: Some(TriggerSourceKind::Chained),
        actor_type: Some(TriggerActorType::System),
        actor_replica_id: None,
        actor_display_name: Some("chained".into()),
        request_host: None,
        request_ip: None,
        metadata: runinator_models::json!({
            "chained_from_run_id": source_run_id,
            "trigger_id": trigger_id,
            "pipeline_chain_depth": depth,
        }),
    };
    create_and_start_pipeline_run(db, &pipeline, parameters, provenance).await?;
    Ok(())
}

/// does a source terminal status satisfy the trigger's `on` selector (success/failure/complete).
fn pipeline_chain_status_matches(trigger: &PipelineTrigger, status: WorkflowStatus) -> bool {
    let on = trigger
        .configuration
        .get("on")
        .and_then(Value::as_str)
        .unwrap_or("success");
    match on {
        // a manual cancel is deliberately excluded from `failure` so a cancel does not cascade.
        "failure" => matches!(status, WorkflowStatus::Failed | WorkflowStatus::TimedOut),
        "complete" => status.is_terminal(),
        _ => status == WorkflowStatus::Succeeded,
    }
}

/// the display name of a workflow run's workflow, from its snapshot or a fetch.
async fn workflow_run_name<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<String, SendableError> {
    if let Some(snapshot) = run.workflow_snapshot.as_ref() {
        return Ok(snapshot.name.clone());
    }
    Ok(db
        .fetch_workflow(run.workflow_id)
        .await?
        .map(|workflow| workflow.name)
        .unwrap_or_default())
}
