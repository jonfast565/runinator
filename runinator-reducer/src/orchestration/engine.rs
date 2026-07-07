use super::context::runtime_context;
use super::handler::{NodeHandler, NodeHandlerContext};
use super::*;
use super::{
    action, approval, assert, audit, await_run, barrier, basic, checkpoint, circuit_breaker,
    collect, compensation, control_flow, debounce, event_source, gate, input, map, mutex, output,
    signal, subflow, throttle, transform, transitions, wait,
};
use uuid::Uuid;

const MAX_INLINE_WORKFLOW_STEPS: usize = 64;

#[tracing::instrument(
    skip_all,
    fields(run_id = %ready_node.workflow_run_id, node_id = %ready_node.node_id)
)]
pub async fn process_ready_node<T: DatabaseImpl>(
    db: &T,
    ready_node: &ReadyNodeRecord,
) -> Result<ReadyNodeDisposition, SendableError> {
    let Some(mut workflow_run) = db.fetch_workflow_run(ready_node.workflow_run_id).await? else {
        tracing::warn!("ready node references a workflow run that no longer exists");
        return Ok(ReadyNodeDisposition::Complete);
    };
    if workflow_run.status == WorkflowStatus::Queued {
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(ready_node.node_id.clone()),
            None,
            Some("Workflow run claimed from ready queue".into()),
        )
        .await?;
        workflow_run.status = WorkflowStatus::Running;
        workflow_run.active_node_id = Some(ready_node.node_id.clone());
    }

    for step in 0..MAX_INLINE_WORKFLOW_STEPS {
        let before = WorkflowProgressKey::from_run(db, workflow_run.id).await?;
        let disposition = process_workflow_run_step(db, workflow_run.clone()).await?;
        let Some(next_run) = db.fetch_workflow_run(workflow_run.id).await? else {
            return Ok(ReadyNodeDisposition::Complete);
        };
        let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        let after = WorkflowProgressKey::from_parts(&next_run, &node_runs);
        let awaits_worker = active_node_awaits_worker(db, &next_run).await?;
        if disposition == ReadyNodeDisposition::KeepClaim
            || should_stop_inline_progress(&next_run, &node_runs, awaits_worker)
            || before == after
        {
            tracing::debug!(
                inline_steps = step + 1,
                disposition = ?disposition,
                active_node_id = ?next_run.active_node_id,
                status = ?next_run.status,
                "workflow run step settled"
            );
            transitions::maybe_wake_subflow_parent(db, &next_run).await?;
            // a run that acquired a named mutex holds it for the rest of the run; release on any
            // terminal state so the next waiter can acquire. no-op for runs holding no lease.
            if next_run.status.is_terminal() {
                mutex::release_run_mutexes(db, next_run.id).await?;
            }
            return Ok(disposition);
        }
        workflow_run = next_run;
    }

    tracing::warn!(
        max_inline_steps = MAX_INLINE_WORKFLOW_STEPS,
        active_node_id = ?workflow_run.active_node_id,
        "inline workflow progress limit exhausted; blocking run"
    );
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Blocked,
        workflow_run.active_node_id.clone(),
        None,
        Some("Inline workflow progress limit exhausted".into()),
    )
    .await?;
    Ok(ReadyNodeDisposition::Complete)
}

async fn process_workflow_run_step<T: DatabaseImpl>(
    db: &T,
    workflow_run: WorkflowRun,
) -> Result<ReadyNodeDisposition, SendableError> {
    if workflow_run.status.is_terminal() || workflow_run.status == WorkflowStatus::Paused {
        return Ok(ReadyNodeDisposition::Complete);
    }
    let workflow = match workflow_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => db
            .fetch_workflow(workflow_run.workflow_id)
            .await?
            .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(workflow_run.workflow_id))?,
    };
    let (start, nodes) = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
    let active_node_id = workflow_run
        .active_node_id
        .clone()
        .unwrap_or_else(|| start.clone());
    // a map fan-out child stops when its body returns to the controlling map node, instead of
    // re-entering the map and fanning out again. finalize the child so it wakes the parent.
    if let Some(child) = workflow_run
        .state
        .get("map_child")
        .and_then(|value| MapChildState::from_wire_value(value).ok())
        && active_node_id == child.stop_node
    {
        map::finalize_map_child(db, &workflow_run, child, &node_runs).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    // workflow-level `watch` guards: re-evaluated on every drive (including while parked), so a
    // state change a fixed checkpoint would miss still pre-empts the active node and jumps to the
    // handler. fires at most once per run.
    if let Some(handler) =
        evaluate_watches(db, &workflow, &workflow_run, &node_runs, &active_node_id).await?
    {
        tracing::info!(active_node_id = %active_node_id, handler = %handler, "watch guard fired");
        let mut run_state = WorkflowRunState::from_state(&workflow_run.state);
        run_state.watch_fired = true;
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(handler.clone()),
            Some(run_state.to_state()),
            Some(format!("watch guard fired; jumping to {handler}")),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    let Some(node) = nodes.iter().find(|node| node.id == active_node_id) else {
        tracing::error!(active_node_id = %active_node_id, "active workflow node is missing from the graph");
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Failed,
            Some(active_node_id),
            None,
            Some("Active workflow node is missing".into()),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    };
    let latest = context::latest_node_run(&node_runs, &active_node_id).cloned();
    if node.skipped {
        basic::process_skipped_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // enforce the reentry safety bound at runtime: a `while`/`until`/poll loop header (or any
    // reentry-enabled node forming a bounded cycle) that has already been visited `max_visits` times
    // exits via `on_exhausted` instead of looping again. without this a loop whose condition never
    // goes false would spin forever, parking on each iteration. only checked when entering the node
    // fresh, never while a prior visit is still in flight.
    if reentry_exhausted(node, &node_runs, latest.as_ref()) {
        match node.reentry.on_exhausted.as_ref() {
            Some(target) => {
                tracing::info!(
                    node_id = %node.id,
                    max_visits = node.reentry.max_visits,
                    target = target.as_str(),
                    "reentry max_visits exhausted; exiting to on_exhausted target"
                );
                db.update_workflow_run_status(
                    workflow_run.id,
                    WorkflowStatus::Running,
                    Some(target.as_str().to_string()),
                    None,
                    Some(format!("reentry_exhausted:{}", node.id)),
                )
                .await?;
            }
            None => {
                tracing::warn!(
                    node_id = %node.id,
                    max_visits = node.reentry.max_visits,
                    "reentry max_visits exhausted with no on_exhausted target; blocking node"
                );
                transitions::block_node(
                    db,
                    &workflow_run,
                    node,
                    "Reentry max_visits exhausted with no on_exhausted target",
                )
                .await?;
            }
        }
        return Ok(ReadyNodeDisposition::Complete);
    }

    let ctx = NodeHandlerContext {
        db,
        workflow: &workflow,
        workflow_run: &workflow_run,
        node,
        latest: latest.as_ref(),
        node_runs: &node_runs,
        nodes: &nodes,
    };

    tracing::debug!(node_id = %node.id, kind = ?node.kind, "dispatching to node handler");
    let disposition = match &node.kind {
        WorkflowNodeKind::Start => basic::StartHandler.process(&ctx).await?,
        WorkflowNodeKind::Action => action::ActionHandler.process(&ctx).await?,
        WorkflowNodeKind::Wait => wait::WaitHandler.process(&ctx).await?,
        WorkflowNodeKind::Condition => basic::ConditionHandler.process(&ctx).await?,
        WorkflowNodeKind::Switch => basic::SwitchHandler.process(&ctx).await?,
        WorkflowNodeKind::Toggle => basic::ToggleHandler.process(&ctx).await?,
        WorkflowNodeKind::Percentage => basic::PercentageHandler.process(&ctx).await?,
        WorkflowNodeKind::Output => output::OutputHandler.process(&ctx).await?,
        WorkflowNodeKind::Input => input::InputHandler.process(&ctx).await?,
        WorkflowNodeKind::Config => basic::ConfigHandler.process(&ctx).await?,
        WorkflowNodeKind::End => basic::EndHandler.process(&ctx).await?,
        WorkflowNodeKind::Fail => compensation::FailHandler.process(&ctx).await?,
        WorkflowNodeKind::Loop => control_flow::LoopHandler.process(&ctx).await?,
        WorkflowNodeKind::Parallel => control_flow::ParallelHandler.process(&ctx).await?,
        WorkflowNodeKind::Join => control_flow::JoinHandler.process(&ctx).await?,
        WorkflowNodeKind::Map => map::MapHandler.process(&ctx).await?,
        WorkflowNodeKind::Race => control_flow::RaceHandler.process(&ctx).await?,
        WorkflowNodeKind::Try => control_flow::TryHandler.process(&ctx).await?,
        WorkflowNodeKind::Approval => approval::ApprovalHandler.process(&ctx).await?,
        WorkflowNodeKind::Gate => gate::GateHandler.process(&ctx).await?,
        WorkflowNodeKind::Signal => signal::SignalHandler.process(&ctx).await?,
        WorkflowNodeKind::Subflow => subflow::SubflowHandler.process(&ctx).await?,
        WorkflowNodeKind::Assert => assert::AssertHandler.process(&ctx).await?,
        WorkflowNodeKind::Transform => transform::TransformHandler.process(&ctx).await?,
        WorkflowNodeKind::Audit => audit::AuditHandler.process(&ctx).await?,
        WorkflowNodeKind::Checkpoint => checkpoint::CheckpointHandler.process(&ctx).await?,
        WorkflowNodeKind::Mutex => mutex::MutexHandler.process(&ctx).await?,
        WorkflowNodeKind::Throttle => throttle::ThrottleHandler.process(&ctx).await?,
        WorkflowNodeKind::AwaitRun => await_run::AwaitRunHandler.process(&ctx).await?,
        WorkflowNodeKind::Debounce => debounce::DebounceHandler.process(&ctx).await?,
        WorkflowNodeKind::Collect => collect::CollectHandler.process(&ctx).await?,
        WorkflowNodeKind::Barrier => barrier::BarrierHandler.process(&ctx).await?,
        WorkflowNodeKind::CircuitBreaker => {
            circuit_breaker::CircuitBreakerHandler.process(&ctx).await?
        }
        WorkflowNodeKind::EventSource => event_source::EventSourceHandler.process(&ctx).await?,
    };
    Ok(disposition)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WorkflowProgressKey {
    status: WorkflowStatus,
    active_node_id: Option<String>,
    node_count: usize,
    latest_active_node_run_id: Option<Uuid>,
    latest_active_node_status: Option<WorkflowStatus>,
}

impl WorkflowProgressKey {
    async fn from_run<T: DatabaseImpl>(
        db: &T,
        workflow_run_id: Uuid,
    ) -> Result<Self, SendableError> {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Err(crate::errors::WORKFLOW_RUN_NOT_FOUND.error(workflow_run_id));
        };
        let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
        Ok(Self::from_parts(&run, &nodes))
    }

    fn from_parts(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Self {
        let latest_active = workflow_run
            .active_node_id
            .as_deref()
            .and_then(|active| context::latest_node_run(node_runs, active));
        Self {
            status: workflow_run.status,
            active_node_id: workflow_run.active_node_id.clone(),
            node_count: node_runs.len(),
            latest_active_node_run_id: latest_active.map(|run| run.id),
            latest_active_node_status: latest_active.map(|run| run.status),
        }
    }
}

// completed visits to a reentry-enabled node. each visit records exactly one node run for the node,
// and the bound is only consulted when entering fresh (no in-flight run), so every counted run is a
// finished iteration.
fn reentry_visits(node: &WorkflowNode, node_runs: &[WorkflowNodeRun]) -> i64 {
    node_runs
        .iter()
        .filter(|run| run.node_id == node.id)
        .count() as i64
}

// true when a reentry-bounded node should exit via its safety bound instead of looping again. only
// fires on a fresh entry (no in-flight run for the node), so an iteration still awaiting a worker is
// never abandoned mid-flight.
pub(super) fn reentry_exhausted(
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
    latest: Option<&WorkflowNodeRun>,
) -> bool {
    let entering_fresh = latest.map_or(true, |run| run.status.is_terminal());
    entering_fresh
        && node.reentry.enabled
        && node.reentry.max_visits > 0
        && reentry_visits(node, node_runs) >= node.reentry.max_visits
}

fn should_stop_inline_progress(
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
    active_node_awaits_worker: bool,
) -> bool {
    if workflow_run.status.is_terminal()
        || matches!(
            workflow_run.status,
            WorkflowStatus::DebugPaused
                | WorkflowStatus::Paused
                | WorkflowStatus::Waiting
                | WorkflowStatus::ApprovalRequired
                | WorkflowStatus::Blocked
        )
    {
        return true;
    }

    // a re-entrant control node (loop/map/race/parallel) keeps its node-run `Running` while it
    // iterates or fans out; that is not a park, so the inline loop must keep processing it. only an
    // action node with a `Running` run is genuinely waiting on a worker that will not complete inline.
    if !active_node_awaits_worker {
        return false;
    }
    let Some(active_node_id) = workflow_run.active_node_id.as_deref() else {
        return false;
    };
    context::latest_node_run(node_runs, active_node_id).is_some_and(|run| {
        matches!(
            run.status,
            WorkflowStatus::Running | WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired
        )
    })
}

/// evaluate the workflow's `metadata.watches` guards against the live run context. returns the
/// handler node id of the first guard whose condition holds, or `None`. skips evaluation once a
/// guard has already fired (`state.watch_fired`) and never redirects to the node already active.
async fn evaluate_watches<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
    active_node_id: &str,
) -> Result<Option<String>, SendableError> {
    let Some(watches) = workflow
        .definition
        .metadata
        .pointer("/watches")
        .and_then(|value| value.as_array())
    else {
        return Ok(None);
    };
    if watches.is_empty() || WorkflowRunState::from_state(&workflow_run.state).watch_fired {
        return Ok(None);
    }
    let context = runtime_context(db, workflow_run, node_runs).await;
    for watch in watches {
        let (Some(condition), Some(handler)) = (
            watch.get("condition"),
            watch.get("handler").and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        if handler == active_node_id {
            continue;
        }
        if runinator_workflows::evaluate_condition(condition, &context).unwrap_or(false) {
            return Ok(Some(handler.to_string()));
        }
    }
    Ok(None)
}

/// true when the run's active node is an action node, the one node kind that parks the run `Running`
/// awaiting a worker result that will not arrive inline. control nodes re-enter inline instead.
async fn active_node_awaits_worker<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<bool, SendableError> {
    let Some(active_node_id) = run.active_node_id.as_deref() else {
        return Ok(false);
    };
    let workflow = match run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => match db.fetch_workflow(run.workflow_id).await? {
            Some(workflow) => workflow,
            None => return Ok(false),
        },
    };
    let Ok((_, nodes)) = runinator_workflows::validate_workflow(&workflow) else {
        return Ok(false);
    };
    Ok(nodes
        .iter()
        .find(|node| node.id == active_node_id)
        .is_some_and(|node| {
            // a pure `std.run` compute node reduces in-process, so it never parks awaiting a worker.
            node.kind == WorkflowNodeKind::Action && !compute::is_inprocess_compute(node)
        }))
}
