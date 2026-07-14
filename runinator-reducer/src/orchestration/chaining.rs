use super::*;
use runinator_models::workflows::{WorkflowTrigger, WorkflowTriggerKind};

// max hops in a chain of on-completion triggers before we stop, to bound accidental A->B->A loops.
const MAX_CHAIN_DEPTH: i64 = 32;

/// when a top-level workflow run reaches a terminal state, start any workflows chained to it via
/// `on_success` / `on_failure` / `on_complete` triggers. fire-and-forget: each target starts as a
/// fresh top-level run and no data flows back. deduped per (trigger, source-run) so a re-drive of
/// the same terminal run never double-starts a target.
pub(super) async fn maybe_start_chained_workflows<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<(), SendableError> {
    if !run.status.is_terminal() {
        return Ok(());
    }
    // only top-level runs chain; subflow/map children must not fan out further chains.
    if run.state.get("subflow_parent").is_some() || run.state.get("map_child").is_some() {
        return Ok(());
    }
    let depth = run
        .trigger_metadata
        .get("chain_depth")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if depth >= MAX_CHAIN_DEPTH {
        tracing::warn!(
            run_id = %run.id,
            depth,
            "chained workflow depth limit reached; not chaining further"
        );
        return Ok(());
    }

    for trigger in db.fetch_workflow_triggers(run.workflow_id).await? {
        if trigger.kind != WorkflowTriggerKind::Chained || !trigger.enabled {
            continue;
        }
        let Some(trigger_id) = trigger.id else {
            continue;
        };
        if !chain_status_matches(&trigger, run.status) {
            continue;
        }
        let Some(target_name) = trigger
            .configuration
            .get("target_workflow")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        // exactly-once per (trigger, source run): only the caller that records the firing starts.
        if !db
            .try_record_trigger_firing(trigger_id, run.id.to_string())
            .await?
        {
            continue;
        }
        start_chained_run(db, &trigger, &target_name, run, depth + 1).await?;
    }
    Ok(())
}

/// does the source run's terminal status match the trigger's `on` selector.
fn chain_status_matches(trigger: &WorkflowTrigger, status: WorkflowStatus) -> bool {
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

/// create the target run, stamp chaining provenance, and enqueue its start node.
async fn start_chained_run<T: DatabaseImpl>(
    db: &T,
    trigger: &WorkflowTrigger,
    target_name: &str,
    source_run: &WorkflowRun,
    depth: i64,
) -> Result<(), SendableError> {
    let target = db
        .fetch_workflow_by_name(target_name.to_string())
        .await?
        .ok_or_else(|| crate::errors::CHAIN_TARGET_UNRESOLVED.error(target_name))?;
    let Some(target_id) = target.id else {
        return Err(crate::errors::CHAIN_TARGET_UNRESOLVED.error(target_name));
    };
    let parameters = trigger
        .configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let state = runinator_models::json!({ "control": { "pause_requested": false } });
    let run = db
        .create_workflow_run(
            target_id,
            target,
            parameters,
            state,
            None,
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Chained),
                actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("chained".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({
                    "chained_from_run_id": source_run.id,
                    "chained_from_workflow_id": source_run.workflow_id,
                    "trigger_id": trigger.id,
                    "chain_depth": depth,
                }),
            },
        )
        .await?;
    // enqueue the target's start node so the reducer drives it (mirrors create_subflow_run).
    if let Some(snapshot) = run.workflow_snapshot.as_ref() {
        let (start, _) = runinator_workflows::parse_nodes(snapshot)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let event = NewOrchestrationEvent::new(
            run.id,
            Some(start.clone()),
            "workflow_run_created",
            runinator_models::json!({ "workflow_id": run.workflow_id, "node_id": start }),
        );
        db.enqueue_ready_node(event, start, Utc::now()).await?;
    }
    tracing::info!(
        source_run_id = %source_run.id,
        target_run_id = %run.id,
        target = target_name,
        "started chained workflow"
    );
    Ok(())
}
