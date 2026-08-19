use std::collections::{HashMap, HashSet};

use super::*;
use runinator_models::pipelines::{
    Pipeline, PipelineJoinMode, PipelineLink, PipelineLinkSelector, PipelineMember,
    PipelineMemberAttempt, PipelineMemberAttemptStatus, PipelineMemberFailureMode, PipelineRun,
    PipelineTrigger,
};
use runinator_models::replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance};
use runinator_models::schedules::ConcurrencyPolicy;

// max hops in a chain of pipeline-to-pipeline triggers before we stop, bounding accidental cycles.
const MAX_PIPELINE_CHAIN_DEPTH: i64 = 32;

/// create a pipeline run for `pipeline` and start its entry members. used by manual/api starts and by
/// chained-to-pipeline firing. returns the created run (already advanced to `running`, or settled
/// `failed` when the pipeline has no entry members).
pub async fn create_and_start_pipeline_run<T: ReducerStore>(
    db: &T,
    pipeline: &Pipeline,
    parameters: Value,
    provenance: WorkflowRunProvenance,
) -> Result<PipelineRun, SendableError> {
    let Some(pipeline_id) = pipeline.id else {
        return Err(crate::errors::PIPELINE_NOT_FOUND.error("pipeline is missing an id"));
    };
    if !pipeline.graph.is_current() {
        return Err(crate::errors::PIPELINE_NOT_FOUND.error(format!(
            "pipeline {pipeline_id} requires source pack reimport"
        )));
    }
    let existing = db.fetch_pipeline_runs_for_concurrency(pipeline_id).await?;
    let active = existing
        .iter()
        .filter(|run| run.status.is_active() && run.status != WorkflowStatus::Queued)
        .count() as i64;
    let concurrency = pipeline.concurrency;
    if concurrency.max_concurrent_runs > 0 && active >= concurrency.max_concurrent_runs {
        match concurrency.on_conflict {
            ConcurrencyPolicy::Skip => {
                return Err(crate::errors::PIPELINE_CONCURRENCY_REJECTED
                    .error("pipeline concurrency limit reached"));
            }
            ConcurrencyPolicy::CancelPrevious => {
                for prior in existing.iter().filter(|run| run.status.is_active()) {
                    for member in db.fetch_workflow_runs_for_pipeline_run(prior.id).await? {
                        if member.status.is_active() {
                            db.update_workflow_run_status(
                                member.id,
                                WorkflowStatus::Canceled,
                                None,
                                None,
                                Some("Canceled by pipeline concurrency policy".into()),
                            )
                            .await?;
                        }
                    }
                    cancel_open_member_attempts(db, prior.id).await?;
                    db.update_pipeline_run_status(
                        prior.id,
                        WorkflowStatus::Canceled,
                        None,
                        Some("Canceled by newer pipeline run".into()),
                    )
                    .await?;
                }
            }
            ConcurrencyPolicy::Queue => {
                let state = runinator_models::json!({ "trigger": provenance.metadata.clone() });
                return db
                    .create_pipeline_run(
                        pipeline_id,
                        pipeline.clone(),
                        parameters,
                        state,
                        provenance,
                    )
                    .await;
            }
            ConcurrencyPolicy::Allow => {}
        }
    }
    let state = runinator_models::json!({ "trigger": provenance.metadata.clone() });
    let run = db
        .create_pipeline_run(pipeline_id, pipeline.clone(), parameters, state, provenance)
        .await?;
    if start_pipeline_run(db, &run).await? == PipelineStartOutcome::Skipped {
        return Err(crate::errors::PIPELINE_CONCURRENCY_REJECTED
            .error("pipeline concurrency limit reached"));
    }
    Ok(run)
}

async fn cancel_open_member_attempts<T: ReducerStore>(
    db: &T,
    pipeline_run_id: Uuid,
) -> Result<(), SendableError> {
    for attempt in db.fetch_pipeline_member_attempts(pipeline_run_id).await? {
        if !attempt.status.is_terminal() {
            db.update_pipeline_member_attempt(
                attempt.id,
                PipelineMemberAttemptStatus::Canceled,
                attempt.result,
                Some("Canceled by pipeline concurrency policy".into()),
            )
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStartOutcome {
    Started,
    Queued,
    Skipped,
}

/// start a pipeline run's entry members. the pipeline_runs row already exists (queued). computes the
/// entry members (members with no in-pipeline inbound link), starts each as a tagged workflow run, and
/// flips the pipeline run to `running`; settles `failed` when there are no entry members to start.
pub async fn start_pipeline_run<T: ReducerStore>(
    db: &T,
    run: &PipelineRun,
) -> Result<PipelineStartOutcome, SendableError> {
    let pipeline = match run.pipeline_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => db
            .fetch_pipeline(run.pipeline_id)
            .await?
            .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(run.pipeline_id))?,
    };
    if !pipeline.graph.is_current() {
        db.update_pipeline_run_status(
            run.id,
            WorkflowStatus::Failed,
            None,
            Some("Pipeline graph requires source pack reimport".into()),
        )
        .await?;
        return Err(crate::errors::PIPELINE_NOT_FOUND.error(run.pipeline_id));
    }
    let existing = db
        .fetch_pipeline_runs_for_concurrency(run.pipeline_id)
        .await?;
    let active = existing
        .iter()
        .filter(|candidate| {
            candidate.id != run.id
                && candidate.status.is_active()
                && candidate.status != WorkflowStatus::Queued
        })
        .count() as i64;
    let concurrency = pipeline.concurrency;
    if concurrency.max_concurrent_runs > 0 && active >= concurrency.max_concurrent_runs {
        match concurrency.on_conflict {
            ConcurrencyPolicy::Skip => {
                db.discard_queued_pipeline_run(run.id).await?;
                return Ok(PipelineStartOutcome::Skipped);
            }
            ConcurrencyPolicy::Queue => return Ok(PipelineStartOutcome::Queued),
            ConcurrencyPolicy::CancelPrevious => {
                for prior in existing
                    .iter()
                    .filter(|candidate| candidate.id != run.id && candidate.status.is_active())
                {
                    for member in db.fetch_workflow_runs_for_pipeline_run(prior.id).await? {
                        if member.status.is_active() {
                            db.update_workflow_run_status(
                                member.id,
                                WorkflowStatus::Canceled,
                                None,
                                None,
                                Some("Canceled by pipeline concurrency policy".into()),
                            )
                            .await?;
                        }
                    }
                    cancel_open_member_attempts(db, prior.id).await?;
                    db.update_pipeline_run_status(
                        prior.id,
                        WorkflowStatus::Canceled,
                        None,
                        Some("Canceled by newer pipeline run".into()),
                    )
                    .await?;
                }
            }
            ConcurrencyPolicy::Allow => {}
        }
    }
    let entry = pipeline_entry_members(&pipeline);
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
    for member in entry {
        let _ = start_member_run(db, run, member, run.parameters.clone(), 1).await?;
    }
    advance_pipeline_graph(db, run, &pipeline).await?;
    settle_pipeline_run_if_complete(db, run, &pipeline).await?;
    Ok(PipelineStartOutcome::Started)
}

/// Retry a failed/timed-out frontier member inside the same pipeline run.
pub async fn retry_pipeline_member<T: ReducerStore>(
    db: &T,
    pipeline_run_id: Uuid,
    member_key: &str,
    parameter_override: Value,
) -> Result<PipelineMemberAttempt, SendableError> {
    let pipeline_run = db
        .fetch_pipeline_run(pipeline_run_id)
        .await?
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(pipeline_run_id))?;
    let pipeline = pipeline_for_run(db, &pipeline_run)
        .await?
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(pipeline_run.pipeline_id))?;
    let member = pipeline
        .graph
        .members
        .iter()
        .find(|member| member.key == member_key)
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(member_key))?;
    let attempts = db.fetch_pipeline_member_attempts(pipeline_run_id).await?;
    let latest = attempts
        .iter()
        .filter(|attempt| attempt.member_key == member_key)
        .max_by_key(|attempt| attempt.attempt)
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(member_key))?;
    if !matches!(
        latest.status,
        PipelineMemberAttemptStatus::Failed | PipelineMemberAttemptStatus::TimedOut
    ) {
        return Err(crate::errors::PIPELINE_MEMBER_NOT_RETRYABLE
            .error(format!("member {member_key} is not retryable")));
    }
    let mut descendants = HashSet::new();
    let mut frontier = vec![member_key];
    while let Some(source) = frontier.pop() {
        for link in pipeline
            .graph
            .links
            .iter()
            .filter(|link| link.enabled && link.from == source)
        {
            if descendants.insert(link.to.as_str()) {
                frontier.push(link.to.as_str());
            }
        }
    }
    if attempts.iter().any(|attempt| {
        descendants.contains(attempt.member_key.as_str()) && attempt.workflow_run_id.is_some()
    }) {
        return Err(crate::errors::PIPELINE_MEMBER_NOT_RETRYABLE.error(format!(
            "member {member_key} is no longer on the retry frontier"
        )));
    }
    for descendant in &descendants {
        db.delete_unstarted_pipeline_member_attempts(pipeline_run_id, (*descendant).to_string())
            .await?;
    }
    let mut parameters = latest.parameters.clone();
    if let (Some(base), Some(overlay)) =
        (parameters.as_object_mut(), parameter_override.as_object())
    {
        for (key, value) in overlay {
            base.insert(key.clone(), value.clone());
        }
    }
    db.reopen_pipeline_run(pipeline_run_id, "Pipeline member retry started".into())
        .await?;
    let _ = start_member_run(db, &pipeline_run, member, parameters, latest.attempt + 1).await?;
    advance_pipeline_graph(db, &pipeline_run, &pipeline).await?;
    settle_pipeline_run_if_complete(db, &pipeline_run, &pipeline).await?;
    db.fetch_pipeline_member_attempts(pipeline_run_id)
        .await?
        .into_iter()
        .filter(|attempt| attempt.member_key == member_key)
        .max_by_key(|attempt| attempt.attempt)
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(member_key))
}

/// Resolve entry members: graph members with no enabled inbound link.
fn pipeline_entry_members(pipeline: &Pipeline) -> Vec<&PipelineMember> {
    let downstream: HashSet<&str> = pipeline
        .graph
        .links
        .iter()
        .filter(|link| link.enabled)
        .map(|link| link.to.as_str())
        .collect();
    pipeline
        .graph
        .members
        .iter()
        .filter(|member| !downstream.contains(member.key.as_str()))
        .collect()
}

/// start a single member workflow run tagged with the owning pipeline run and enqueue its start node.
async fn start_member_run<T: ReducerStore>(
    db: &T,
    pipeline_run: &PipelineRun,
    member: &PipelineMember,
    parameters: Value,
    attempt_number: i64,
) -> Result<bool, SendableError> {
    let workflow_id = member.workflow_id;
    let Some(attempt) = db
        .create_pipeline_member_attempt(
            pipeline_run.id,
            member.key.clone(),
            workflow_id,
            attempt_number,
            parameters.clone(),
        )
        .await?
    else {
        return Ok(false);
    };
    let Some(snapshot) = db.fetch_workflow(workflow_id).await? else {
        db.update_pipeline_member_attempt(
            attempt.id,
            PipelineMemberAttemptStatus::Failed,
            Value::Null,
            Some("Pipeline member workflow no longer exists".into()),
        )
        .await?;
        return Ok(true);
    };
    if let Err(err) = snapshot.input_type.validate_value(&parameters) {
        db.update_pipeline_member_attempt(
            attempt.id,
            PipelineMemberAttemptStatus::Failed,
            Value::Null,
            Some(err.to_string()),
        )
        .await?;
        return Ok(true);
    }
    let state = runinator_models::json!({ "control": { "pause_requested": false } });
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot.clone(),
            parameters,
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
    db.bind_pipeline_member_attempt_run(attempt.id, run.id)
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
    Ok(true)
}

/// the failure mode governing `workflow_id`'s membership in `pipeline`: its own override, or the
/// pipeline's default when it has none. `None` (pipeline unresolved) falls back to `Continue`, which
/// reproduces the pre-failure-mode behavior exactly.
fn member_failure_mode(
    pipeline: Option<&Pipeline>,
    workflow_id: Uuid,
) -> PipelineMemberFailureMode {
    let Some(pipeline) = pipeline else {
        return PipelineMemberFailureMode::default();
    };
    pipeline
        .graph
        .members
        .iter()
        .find(|member| member.workflow_id == workflow_id)
        .map(|member| member.failure_mode)
        .unwrap_or(pipeline.defaults.default_failure_mode)
}

/// the pipeline snapshot to classify failure modes against: the run's own snapshot when present
/// (fixing the ruleset to what was true when the pipeline run started), else a fresh fetch.
async fn pipeline_for_run<T: ReducerStore>(
    db: &T,
    pipeline_run: &PipelineRun,
) -> Result<Option<Pipeline>, SendableError> {
    match pipeline_run.pipeline_snapshot.clone() {
        Some(snapshot) => Ok(Some(snapshot)),
        None => db.fetch_pipeline(pipeline_run.pipeline_id).await,
    }
}

/// pause a pipeline run (`approval_required`) and record which member's failure raised the inquiry,
/// so the ws inquiry-resolution endpoint and the command center can find it on `state.pending_inquiry`.
async fn pause_pipeline_run_for_inquiry<T: ReducerStore>(
    db: &T,
    pipeline_run: &PipelineRun,
    member_run: &WorkflowRun,
) -> Result<(), SendableError> {
    let mut state = pipeline_run.state.clone();
    if let Some(map) = state.as_object_mut() {
        map.insert(
            "pending_inquiry".to_string(),
            runinator_models::json!({
                "member_run_id": member_run.id,
                "workflow_id": member_run.workflow_id,
                "status": member_run.status.as_str(),
                "raised_at": Utc::now(),
            }),
        );
    }
    db.update_pipeline_run_status(
        pipeline_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(state),
        Some("Awaiting a decision on a member failure (Inquire failure mode)".into()),
    )
    .await
}

/// which way a pending inquiry ([`PipelineMemberFailureMode::Inquire`]) was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineInquiryDecision {
    /// fire the failed member's onward pipeline links (as `Continue` would have) and resume.
    Continue,
    /// settle the pipeline run `failed` now, without firing the failed member's onward links.
    Abort,
}

/// resolve a pipeline run's pending inquiry. errors if the run has no pending inquiry recorded
/// (already resolved, or the run was never paused).
pub async fn resolve_pipeline_run_inquiry<T: ReducerStore>(
    db: &T,
    pipeline_run_id: Uuid,
    decision: PipelineInquiryDecision,
    resolved_by: Option<String>,
    message: Option<String>,
) -> Result<PipelineRun, SendableError> {
    let pipeline_run = db
        .fetch_pipeline_run(pipeline_run_id)
        .await?
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(pipeline_run_id))?;
    let Some(pending) = pipeline_run.state.get("pending_inquiry").cloned() else {
        return Err(crate::errors::PIPELINE_NO_PENDING_INQUIRY.error(pipeline_run_id));
    };
    let member_run_id = pending
        .get("member_run_id")
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or_else(|| crate::errors::PIPELINE_INQUIRY_MEMBER_MISSING.error(pipeline_run_id))?;

    let mut state = pipeline_run.state.clone();
    if let Some(map) = state.as_object_mut() {
        map.remove("pending_inquiry");
        map.insert(
            "last_inquiry_resolution".to_string(),
            runinator_models::json!({
                "decision": if decision == PipelineInquiryDecision::Continue {
                    "continue"
                } else {
                    "abort"
                },
                "resolved_by": resolved_by,
                "message": message,
                "resolved_at": Utc::now(),
            }),
        );
    }

    match decision {
        PipelineInquiryDecision::Abort => {
            db.update_pipeline_run_status(
                pipeline_run_id,
                WorkflowStatus::Failed,
                Some(state),
                Some("Pipeline aborted after an inquiry".into()),
            )
            .await?;
            let mut settled = pipeline_run.clone();
            settled.status = WorkflowStatus::Failed;
            maybe_start_chained_pipelines_from_pipeline(db, &settled).await?;
        }
        PipelineInquiryDecision::Continue => {
            let member_run = db.fetch_workflow_run(member_run_id).await?.ok_or_else(|| {
                crate::errors::PIPELINE_INQUIRY_MEMBER_MISSING.error(pipeline_run_id)
            })?;
            db.update_pipeline_run_status(
                pipeline_run_id,
                WorkflowStatus::Running,
                Some(state),
                None,
            )
            .await?;
            let run_ctx = super::handler::WorkflowRunContext::new(db, &member_run);
            settle_member_attempt(db, &member_run).await?;
            if let Some(pipeline) = pipeline_for_run(db, &pipeline_run).await? {
                advance_pipeline_graph(db, &pipeline_run, &pipeline).await?;
            }
            maybe_settle_pipeline_run(&run_ctx).await?;
        }
    }

    db.fetch_pipeline_run(pipeline_run_id)
        .await?
        .ok_or_else(|| crate::errors::PIPELINE_NOT_FOUND.error(pipeline_run_id))
}

/// when a member workflow run reaches terminal, settle its owning pipeline run if the whole reachable
/// member graph is now terminal. no-op for runs not tagged with a pipeline run, already-settled runs,
/// or a run currently paused on a pending inquiry.
pub(super) async fn maybe_settle_pipeline_run<T: ReducerStore>(
    ctx: &super::handler::WorkflowRunContext<'_, T>,
) -> Result<(), SendableError> {
    let member_run = ctx.workflow_run;
    let Some(pipeline_run_id) = member_run.pipeline_run_id else {
        return Ok(());
    };
    let Some(pipeline_run) = ctx.db.fetch_pipeline_run(pipeline_run_id).await? else {
        return Ok(());
    };
    if pipeline_run.status.is_terminal() {
        return Ok(());
    }
    // paused for a human decision (`Inquire`): settlement resumes once it is resolved.
    if pipeline_run.status == WorkflowStatus::ApprovalRequired {
        return Ok(());
    }
    let Some(pipeline) = pipeline_for_run(ctx.db, &pipeline_run).await? else {
        return Ok(());
    };
    let mode = member_failure_mode(Some(&pipeline), member_run.workflow_id);
    let already_recorded = ctx
        .db
        .fetch_pipeline_member_attempts(pipeline_run_id)
        .await?
        .iter()
        .any(|attempt| {
            attempt.workflow_run_id == Some(member_run.id) && attempt.status.is_terminal()
        });
    if matches!(
        member_run.status,
        WorkflowStatus::Failed | WorkflowStatus::TimedOut
    ) && mode == PipelineMemberFailureMode::Inquire
        && !already_recorded
    {
        pause_pipeline_run_for_inquiry(ctx.db, &pipeline_run, member_run).await?;
        return Ok(());
    }
    settle_member_attempt(ctx.db, member_run).await?;
    advance_pipeline_graph(ctx.db, &pipeline_run, &pipeline).await?;
    settle_pipeline_run_if_complete(ctx.db, &pipeline_run, &pipeline).await
}

async fn settle_pipeline_run_if_complete<T: ReducerStore>(
    db: &T,
    pipeline_run: &PipelineRun,
    pipeline: &Pipeline,
) -> Result<(), SendableError> {
    let attempts = db.fetch_pipeline_member_attempts(pipeline_run.id).await?;
    let latest: HashMap<&str, &PipelineMemberAttempt> =
        attempts.iter().fold(HashMap::new(), |mut map, attempt| {
            if map
                .get(attempt.member_key.as_str())
                .is_none_or(|old: &&PipelineMemberAttempt| old.attempt < attempt.attempt)
            {
                map.insert(attempt.member_key.as_str(), attempt);
            }
            map
        });
    if latest.len() < pipeline.graph.members.len()
        || latest.values().any(|attempt| !attempt.status.is_terminal())
    {
        return Ok(());
    }
    let any_failed = latest.values().any(|attempt| {
        matches!(
            attempt.status,
            PipelineMemberAttemptStatus::Failed | PipelineMemberAttemptStatus::TimedOut
        ) && pipeline
            .graph
            .members
            .iter()
            .find(|member| member.key == attempt.member_key)
            .is_none_or(|member| member.failure_mode != PipelineMemberFailureMode::SilentlyContinue)
    });
    let any_canceled = latest
        .values()
        .any(|attempt| attempt.status == PipelineMemberAttemptStatus::Canceled);
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
    db.update_pipeline_run_status(pipeline_run.id, status, None, message)
        .await?;
    // a settled pipeline can itself be the source of a chained-to-pipeline trigger.
    let mut settled = pipeline_run.clone();
    settled.status = status;
    maybe_start_chained_pipelines_from_pipeline(db, &settled).await?;
    start_next_queued_pipeline_run(db, pipeline).await?;
    Ok(())
}

async fn start_next_queued_pipeline_run<T: ReducerStore>(
    db: &T,
    pipeline: &Pipeline,
) -> Result<(), SendableError> {
    let Some(pipeline_id) = pipeline.id else {
        return Ok(());
    };
    let runs = db.fetch_pipeline_runs_for_concurrency(pipeline_id).await?;
    let active = runs
        .iter()
        .filter(|run| run.status.is_active() && run.status != WorkflowStatus::Queued)
        .count() as i64;
    let limit = pipeline.concurrency.max_concurrent_runs;
    if limit > 0 && active >= limit {
        return Ok(());
    }
    if let Some(queued) = runs
        .into_iter()
        .find(|run| run.status == WorkflowStatus::Queued)
    {
        let _ = Box::pin(start_pipeline_run(db, &queued)).await?;
    }
    Ok(())
}

async fn settle_member_attempt<T: ReducerStore>(
    db: &T,
    run: &WorkflowRun,
) -> Result<(), SendableError> {
    let attempts = db
        .fetch_pipeline_member_attempts(run.pipeline_run_id.expect("pipeline member"))
        .await?;
    let Some(attempt) = attempts
        .iter()
        .find(|attempt| attempt.workflow_run_id == Some(run.id))
    else {
        return Ok(());
    };
    if attempt.status.is_terminal() {
        return Ok(());
    }
    let result = member_result(db, run, attempt.attempt).await?;
    db.update_pipeline_member_attempt(
        attempt.id,
        attempt_status(run.status),
        result,
        run.message.clone(),
    )
    .await
}

fn attempt_status(status: WorkflowStatus) -> PipelineMemberAttemptStatus {
    match status {
        WorkflowStatus::Succeeded => PipelineMemberAttemptStatus::Succeeded,
        WorkflowStatus::TimedOut => PipelineMemberAttemptStatus::TimedOut,
        WorkflowStatus::Canceled => PipelineMemberAttemptStatus::Canceled,
        WorkflowStatus::Failed => PipelineMemberAttemptStatus::Failed,
        WorkflowStatus::ApprovalRequired => PipelineMemberAttemptStatus::ApprovalRequired,
        WorkflowStatus::Waiting
        | WorkflowStatus::InputRequired
        | WorkflowStatus::Paused
        | WorkflowStatus::DebugPaused => PipelineMemberAttemptStatus::Waiting,
        WorkflowStatus::Queued => PipelineMemberAttemptStatus::Queued,
        WorkflowStatus::Running | WorkflowStatus::Blocked => PipelineMemberAttemptStatus::Running,
    }
}

async fn member_result<T: ReducerStore>(
    db: &T,
    run: &WorkflowRun,
    attempt: i64,
) -> Result<Value, SendableError> {
    let node_runs = db.fetch_workflow_node_runs(run.id).await?;
    let artifacts = db.fetch_promoted_workflow_run_artifacts(run.id).await?;
    let output_ids: HashSet<&str> = run
        .workflow_snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .definition
                .nodes
                .iter()
                .filter(|node| node.kind == WorkflowNodeKind::Output)
                .map(|node| node.id.as_str())
                .collect()
        })
        .unwrap_or_default();
    let mut outputs = Map::new();
    let mut results = Map::new();
    for node in node_runs
        .iter()
        .filter(|node| !node.speculative && node.status.is_terminal())
    {
        if let Some(output) = &node.output_json {
            outputs.insert(node.node_id.clone(), output.clone());
            if output_ids.contains(node.node_id.as_str())
                && node.status == WorkflowStatus::Succeeded
            {
                results.insert(
                    node.node_id.clone(),
                    output.get("data").cloned().unwrap_or(Value::Null),
                );
            }
        }
    }
    let result = if results.len() == 1 {
        results.values().next().cloned().unwrap_or(Value::Null)
    } else if results.is_empty() {
        Value::Null
    } else {
        Value::Object(results)
    };
    let duration_ms = run
        .started_at
        .zip(run.finished_at)
        .map(|(started, finished)| (finished - started).num_milliseconds());
    Ok(runinator_models::json!({
        "run_id": run.id, "workflow_id": run.workflow_id, "status": run.status.as_str(), "attempt": attempt,
        "result": result, "outputs": Value::Object(outputs), "artifacts": artifacts,
        "created_at": run.created_at, "started_at": run.started_at, "finished_at": run.finished_at,
        "duration_ms": duration_ms
    }))
}

async fn advance_pipeline_graph<T: ReducerStore>(
    db: &T,
    pipeline_run: &PipelineRun,
    pipeline: &Pipeline,
) -> Result<(), SendableError> {
    loop {
        let attempts = db.fetch_pipeline_member_attempts(pipeline_run.id).await?;
        let latest: HashMap<&str, &PipelineMemberAttempt> =
            attempts.iter().fold(HashMap::new(), |mut map, attempt| {
                if map
                    .get(attempt.member_key.as_str())
                    .is_none_or(|old: &&PipelineMemberAttempt| old.attempt < attempt.attempt)
                {
                    map.insert(attempt.member_key.as_str(), attempt);
                }
                map
            });
        let mut changed = false;
        for member in &pipeline.graph.members {
            if latest.contains_key(member.key.as_str()) {
                continue;
            }
            let inbound = pipeline
                .graph
                .links
                .iter()
                .filter(|link| link.enabled && link.to == member.key)
                .collect::<Vec<_>>();
            if inbound.is_empty() {
                continue;
            }
            let join = pipeline.graph.joins.get(&member.key);
            let terminal_count = inbound
                .iter()
                .filter(|link| {
                    latest
                        .get(link.from.as_str())
                        .is_some_and(|a| a.status.is_terminal())
                })
                .count();
            let mut matches = inbound
                .iter()
                .filter_map(|link| {
                    let attempt = latest.get(link.from.as_str()).copied()?;
                    let source = pipeline
                        .graph
                        .members
                        .iter()
                        .find(|source| source.key == link.from)?;
                    selector_matches(link, attempt.status, source.failure_mode).then_some(attempt)
                })
                .collect::<Vec<_>>();
            matches.sort_by_key(|attempt| attempt.finished_at.unwrap_or(attempt.created_at));
            let ready = match join.map(|join| join.mode) {
                Some(PipelineJoinMode::All) => {
                    terminal_count == inbound.len() && matches.len() == inbound.len()
                }
                Some(PipelineJoinMode::Any) => !matches.is_empty(),
                Some(PipelineJoinMode::FirstSuccess) => matches
                    .iter()
                    .any(|attempt| attempt.status == PipelineMemberAttemptStatus::Succeeded),
                None => matches.len() == 1,
            };
            let impossible = terminal_count == inbound.len() && !ready;
            if ready {
                let source = match join.map(|join| join.mode) {
                    Some(PipelineJoinMode::All) => None,
                    _ => matches.first().copied(),
                };
                let mapping = join
                    .map(|join| &join.parameters)
                    .unwrap_or(&inbound[0].parameters);
                match resolve_member_parameters(&pipeline_run.parameters, mapping, source, &latest)
                {
                    Ok(parameters) => {
                        changed |=
                            start_member_run(db, pipeline_run, member, parameters, 1).await?;
                    }
                    Err(err) => {
                        if let Some(attempt) = db
                            .create_pipeline_member_attempt(
                                pipeline_run.id,
                                member.key.clone(),
                                member.workflow_id,
                                1,
                                pipeline_run.parameters.clone(),
                            )
                            .await?
                        {
                            db.update_pipeline_member_attempt(
                                attempt.id,
                                PipelineMemberAttemptStatus::Failed,
                                Value::Null,
                                Some(err.to_string()),
                            )
                            .await?;
                            changed = true;
                        }
                    }
                }
            } else if impossible {
                if let Some(attempt) = db
                    .create_pipeline_member_attempt(
                        pipeline_run.id,
                        member.key.clone(),
                        member.workflow_id,
                        1,
                        pipeline_run.parameters.clone(),
                    )
                    .await?
                {
                    db.update_pipeline_member_attempt(
                        attempt.id,
                        PipelineMemberAttemptStatus::Skipped,
                        Value::Null,
                        Some("No inbound link satisfied the join".into()),
                    )
                    .await?;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

fn selector_matches(
    link: &PipelineLink,
    status: PipelineMemberAttemptStatus,
    failure_mode: PipelineMemberFailureMode,
) -> bool {
    if matches!(
        status,
        PipelineMemberAttemptStatus::Failed | PipelineMemberAttemptStatus::TimedOut
    ) && failure_mode == PipelineMemberFailureMode::Stop
    {
        return false;
    }
    match link.on {
        PipelineLinkSelector::Success => status == PipelineMemberAttemptStatus::Succeeded,
        PipelineLinkSelector::Failure => matches!(
            status,
            PipelineMemberAttemptStatus::Failed | PipelineMemberAttemptStatus::TimedOut
        ),
        PipelineLinkSelector::Complete => {
            status.is_terminal() && status != PipelineMemberAttemptStatus::Skipped
        }
    }
}

fn resolve_member_parameters(
    pipeline_parameters: &Value,
    mapping: &Value,
    source: Option<&PipelineMemberAttempt>,
    latest: &HashMap<&str, &PipelineMemberAttempt>,
) -> Result<Value, SendableError> {
    let members = latest
        .iter()
        .map(|(key, attempt)| ((*key).to_string(), attempt.result.clone()))
        .collect::<Map>();
    // Pipeline mappings reuse the compute evaluator. WDL lowers `source.*` and `members[...]` as
    // node-output refs, so expose those two pipeline-only roots through the evaluator's `steps`
    // slot while `params.*` continues to resolve through `input`.
    let context = runinator_models::json!({
        "input": pipeline_parameters.clone(),
        "steps": {
            "source": { "output": source.map(|a| a.result.clone()).unwrap_or(Value::Null) },
            "members": { "output": members }
        }
    });
    let resolved = runinator_workflows::resolve_value_refs_pure(mapping, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let mut parameters = pipeline_parameters.clone();
    if let (Some(base), Some(overlay)) = (parameters.as_object_mut(), resolved.as_object()) {
        for (key, value) in overlay {
            base.insert(key.clone(), value.clone());
        }
    } else if !resolved.is_null() {
        parameters = resolved;
    }
    Ok(parameters)
}

/// start any pipelines chained to a terminal workflow run via an enabled `chained` pipeline trigger
/// whose `configuration.source_workflow` matches. deduped per (trigger, source run).
pub(super) async fn maybe_start_chained_pipelines<T: ReducerStore>(
    ctx: &super::handler::WorkflowRunContext<'_, T>,
) -> Result<(), SendableError> {
    let source_run = ctx.workflow_run;
    if !source_run.status.is_terminal() {
        return Ok(());
    }
    // subflow/map children never fan out further chains.
    if source_run.execution_state.subflow_parent.is_some()
        || source_run.execution_state.map_child.is_some()
    {
        return Ok(());
    }
    let source_name = workflow_run_name(ctx.db, source_run).await?;
    let triggers = ctx.db.fetch_enabled_chained_pipeline_triggers().await?;
    for trigger in triggers {
        let matches_source = trigger
            .configuration
            .get("source_workflow")
            .and_then(Value::as_str)
            == Some(source_name.as_str());
        if !matches_source {
            continue;
        }
        start_chained_pipeline(ctx.db, &trigger, source_run.status, source_run.id, 0).await?;
    }
    Ok(())
}

/// start any pipelines chained to a terminal pipeline run via a `chained` pipeline trigger whose
/// `configuration.source_pipeline` matches. bounds cycles with a chain-depth guard.
async fn maybe_start_chained_pipelines_from_pipeline<T: ReducerStore>(
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
async fn start_chained_pipeline<T: ReducerStore>(
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
    Box::pin(create_and_start_pipeline_run(
        db, &pipeline, parameters, provenance,
    ))
    .await?;
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
async fn workflow_run_name<T: ReducerStore>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeStore;
    use runinator_models::workflows::{WorkflowDefinition, WorkflowRunArtifact};

    fn attempt(key: &str, result: Value) -> PipelineMemberAttempt {
        PipelineMemberAttempt {
            id: Uuid::now_v7(),
            pipeline_run_id: Uuid::now_v7(),
            member_key: key.into(),
            workflow_id: Uuid::now_v7(),
            attempt: 1,
            workflow_run_id: Some(Uuid::now_v7()),
            status: PipelineMemberAttemptStatus::Succeeded,
            parameters: Value::Null,
            result,
            message: None,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: Some(Utc::now()),
        }
    }

    #[test]
    fn pipeline_mapping_overlays_params_and_resolves_source_and_members() {
        let source = attempt(
            "Build",
            runinator_models::json!({ "result": { "artifact": "app.tgz" } }),
        );
        let linux = attempt(
            "Linux Build",
            runinator_models::json!({ "result": { "sha": "abc" } }),
        );
        let latest = HashMap::from([("Build", &source), ("Linux Build", &linux)]);
        let mapping = runinator_models::json!({
            "artifact": { "$ref": { "node": "source", "output": ["result", "artifact"] } },
            "linux": { "$ref": { "node": "members", "output": ["Linux Build", "result", "sha"] } },
            "environment": { "$ref": { "params": ["environment"] } }
        });
        let resolved = resolve_member_parameters(
            &runinator_models::json!({ "environment": "prod", "keep": true }),
            &mapping,
            Some(&source),
            &latest,
        )
        .expect("mapping");
        assert_eq!(
            resolved,
            runinator_models::json!({
                "environment": "prod", "keep": true, "artifact": "app.tgz", "linux": "abc"
            })
        );
    }

    #[test]
    fn stop_failure_mode_suppresses_every_outbound_selector() {
        let link = PipelineLink {
            id: Uuid::now_v7(),
            from: "A".into(),
            to: "B".into(),
            on: PipelineLinkSelector::Complete,
            enabled: true,
            parameters: Value::Null,
        };
        assert!(!selector_matches(
            &link,
            PipelineMemberAttemptStatus::Failed,
            PipelineMemberFailureMode::Stop
        ));
        assert!(selector_matches(
            &link,
            PipelineMemberAttemptStatus::Failed,
            PipelineMemberFailureMode::Continue
        ));
    }

    #[tokio::test]
    async fn member_envelope_aggregates_output_data_and_promoted_artifacts() {
        let store = FakeStore::new();
        let workflow_id = Uuid::now_v7();
        let run_id = Uuid::now_v7();
        let workflow: WorkflowDefinition = serde_json::from_value(serde_json::json!({
            "id": workflow_id, "name": "Build", "version": "1.0.0", "enabled": true,
            "definition": { "start": "start", "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "publish" } } },
                { "id": "publish", "kind": "output", "parameters": {}, "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ] }
        })).expect("workflow");
        let run: WorkflowRun = serde_json::from_value(serde_json::json!({
            "id": run_id, "workflow_id": workflow_id, "workflow_snapshot": workflow,
            "status": "succeeded", "active_node_id": "end", "parameters": {}, "state": {},
            "created_at": Utc::now(), "started_at": Utc::now(), "finished_at": Utc::now(), "message": null
        })).expect("run");
        let node: WorkflowNodeRun = serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(), "workflow_run_id": run_id, "node_id": "publish",
            "status": "succeeded", "attempt": 1, "parameters": {},
            "output_json": { "data": { "artifact": "app.tgz" } }, "state": null,
            "transition_reason": null, "created_at": Utc::now(), "started_at": Utc::now(),
            "finished_at": Utc::now(), "message": null, "speculative": false, "cursor_id": null
        }))
        .expect("node run");
        store.insert_node_run(node);
        store.insert_run_artifact(WorkflowRunArtifact {
            id: Uuid::now_v7(),
            workflow_run_id: run_id,
            node_id: "publish".into(),
            artifact_id: Uuid::now_v7(),
            name: "app.tgz".into(),
            mime_type: "application/gzip".into(),
            size_bytes: 42,
            uri: "blob://app.tgz".into(),
            metadata: Value::Null,
            created_at: Utc::now(),
        });
        let envelope = member_result(&store, &run, 2).await.expect("envelope");
        assert_eq!(
            envelope.pointer("/result/artifact").and_then(Value::as_str),
            Some("app.tgz")
        );
        assert_eq!(
            envelope.pointer("/attempt").and_then(Value::as_i64),
            Some(2)
        );
        assert_eq!(
            envelope
                .pointer("/artifacts/0/name")
                .and_then(Value::as_str),
            Some("app.tgz")
        );
    }
}
