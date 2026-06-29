use std::future::Future;

use super::context::{is_reentry_stale, merge_parameters, runtime_context};
use super::handler::{NodeHandler, NodeHandlerContext};
use super::transitions::{arm_node_timeout, retry_or_transition, time_out, timed_out};
use super::*;
use runinator_comm::ActionTarget;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use uuid::Uuid;

const FOREIGN_LANGUAGE_SCOPE: &str = "foreign_languages";

// the session-bound local-files provider runs only on the desktop replica that launched the run.
const LOCAL_PROVIDER: &str = "local";
// how often a node parked for an unavailable desktop worker re-checks for it to reconnect.
const LOCAL_TARGET_POLL_SECONDS: i64 = 5;
// a replica whose heartbeat is older than this is treated as disconnected; mirrors the ws reaper.
const REPLICA_STALE_SECONDS: i64 = 30;

// the routing decision for an action node: dispatch now to a resolved target, or park until the
// bound worker becomes available.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TargetResolution {
    Ready(ActionTarget),
    Park,
}

/// pure routing policy: general-pool providers go to `Any`; the session-bound local provider pins to
/// its launching replica when that replica is live, otherwise parks. split out from the db lookup so
/// the decision is unit-testable.
pub(super) fn target_for(
    provider: &str,
    trigger_replica_id: Option<Uuid>,
    replica_live: bool,
) -> TargetResolution {
    if provider != LOCAL_PROVIDER {
        return TargetResolution::Ready(ActionTarget::Any);
    }
    match trigger_replica_id {
        Some(replica_id) if replica_live => {
            TargetResolution::Ready(ActionTarget::Replica { replica_id })
        }
        _ => TargetResolution::Park,
    }
}

pub(super) fn foreign_language_runtime(language: &str) -> Option<(&'static str, &'static str)> {
    match language {
        "python" | "py" => Some(("python", "python:3.12")),
        "javascript" | "js" | "node" => Some(("javascript", "node:22")),
        "bash" | "sh" => Some(("bash", "bash:5.2")),
        "ruby" | "rb" => Some(("ruby", "ruby:3.3")),
        "perl" | "pl" => Some(("perl", "perl:5.40")),
        "php" => Some(("php", "php:8.3-cli")),
        _ => None,
    }
}

pub(super) fn default_foreign_language_runtime(image: &str) -> Value {
    runinator_models::json!({
        "image": image,
        "setup_script": ""
    })
}

pub(super) async fn process_action_node<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let action = node
        .action
        .as_ref()
        .ok_or_else(|| crate::errors::ACTION_CONFIG_MISSING.error(&node.id))?;
    // a loop body re-entering this node sees the prior iteration's terminal run; treat it as a
    // fresh visit so the action dispatches again instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Running {
            // a dispatched action otherwise waits on its worker result indefinitely; honor the
            // node's timeout so a lost worker or dropped result cannot park the run forever.
            if timed_out(node, node_run) {
                return time_out(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    "Action node timed out",
                    node_runs,
                )
                .await;
            }
            return Ok(());
        }
        // a node parked waiting for its bound desktop worker; honor the timeout, otherwise fall
        // through to re-resolve the target (the worker may have reconnected) reusing this run.
        if node_run.status == WorkflowStatus::Waiting && timed_out(node, node_run) {
            return time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Local worker did not become available",
                node_runs,
            )
            .await;
        }
        if node_run.status.is_terminal() {
            retry_or_transition(
                db,
                workflow_run,
                node,
                node_run,
                node_run.status,
                node_run.output_json.clone(),
                node_run.message.clone(),
                node_runs,
            )
            .await?;
            return Ok(());
        }
    }

    let node_run = match latest
        .filter(|run| matches!(run.status, WorkflowStatus::Queued | WorkflowStatus::Waiting))
    {
        Some(node_run) => node_run.clone(),
        None => {
            db.create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?
        }
    };
    // resolve routing before any observable dispatch: a session-bound action whose desktop worker is
    // not connected parks (and re-checks) instead of being published to a queue no one drains.
    let target = match resolve_action_target(db, workflow_run, action).await? {
        TargetResolution::Ready(target) => target,
        TargetResolution::Park => {
            return park_for_target(db, workflow_run, node, &node_run).await;
        }
    };
    let attempt = node_run.attempt + 1;
    let parameters =
        build_node_parameters(db, workflow, action, node, workflow_run, node_runs).await?;
    let command = build_action_command(
        workflow_run.id,
        &node_run,
        action,
        parameters.clone(),
        target,
    );
    // scope the dedupe key to the attempt: outbox rows persist after publish, so a retry reusing
    // the node run's key would collide with the already-published row and never dispatch again.
    db.enqueue_action_dispatch(
        format!("workflow-node-run:{}:{attempt}", node_run.id),
        command,
    )
    .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(attempt),
        Some(parameters),
        None,
        None,
        Some("action_started".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    // no ready node is pending while the run awaits the worker, so a configured timeout must arm
    // its own wake-up to be checked at the deadline.
    arm_node_timeout(db, workflow_run.id, node).await
}

async fn build_node_parameters<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    action: &WorkflowAction,
    node: &WorkflowNode,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    // an effectful `std.exec` program is interpreted by the worker, not resolved here: ship the
    // program verbatim alongside the full runtime context and the workflow's user-function table so
    // the worker's interpreter can resolve refs/calls (with the effectful library) against it.
    if action.provider == "std" && action.function == "exec" {
        let context = runtime_context(db, workflow_run, node_runs).await;
        let program = action
            .configuration
            .as_value()
            .get("program")
            .cloned()
            .unwrap_or(Value::Null);
        let functions = workflow
            .definition
            .metadata
            .get("functions")
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(
            runinator_models::json!({ "program": program, "context": context, "functions": functions }),
        );
    }
    // foreign compute source is passed verbatim to `std.code`; only the live context is appended.
    if action.provider == "std" && action.function == "code" {
        let mut parameters = action.configuration.as_value().clone();
        let context = runtime_context(db, workflow_run, node_runs).await;
        if let Value::Object(object) = &mut parameters {
            let language = object
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let (canonical_language, default_image) =
                foreign_language_runtime(&language).ok_or_else(|| {
                    crate::errors::COMPUTE_NODE_FAILED.error(format!(
                        "unsupported foreign language '{language}'; supported languages: python, javascript, bash, ruby, perl, php"
                    ))
                })?;
            let runtime =
                crate::config::config_value(db, FOREIGN_LANGUAGE_SCOPE, canonical_language)
                    .await?
                    .unwrap_or_else(|| default_foreign_language_runtime(default_image));
            object.insert("language".into(), Value::String(canonical_language.into()));
            object.insert("context".into(), context);
            object.insert("runtime".into(), runtime);
            return Ok(parameters);
        }
        return Ok(runinator_models::json!({ "context": context }));
    }
    let base = merge_parameters(&action.configuration, &node.parameters);
    let context = runtime_context(db, workflow_run, node_runs).await;
    runinator_workflows::resolve_value_refs(&base, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn build_action_command(
    workflow_run_id: Uuid,
    node_run: &WorkflowNodeRun,
    action: &WorkflowAction,
    parameters: Value,
    target: ActionTarget,
) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id: node_run.id,
        node_id: node_run.node_id.clone(),
        action: action.clone(),
        attempt: node_run.attempt + 1,
        parameters,
        target,
        trace_id: Uuid::now_v7(),
    }
}

/// decide which worker(s) may run this action. general-pool actions go to `Any`; the session-bound
/// local-files provider is pinned to the desktop replica that launched the run, and parks when that
/// replica is not currently connected so the action is never published into a queue no one drains.
async fn resolve_action_target<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    action: &WorkflowAction,
) -> Result<TargetResolution, SendableError> {
    // only consult the registry when a local action actually has a launching replica to check.
    let replica_live = match (
        action.provider == LOCAL_PROVIDER,
        workflow_run.trigger_actor_replica_id,
    ) {
        (true, Some(replica_id)) => replica_is_live(db, replica_id).await?,
        _ => false,
    };
    Ok(target_for(
        &action.provider,
        workflow_run.trigger_actor_replica_id,
        replica_live,
    ))
}

/// whether a worker replica has heartbeated recently enough to receive work.
async fn replica_is_live<T: DatabaseImpl>(db: &T, replica_id: Uuid) -> Result<bool, SendableError> {
    let stale_before = Utc::now() - chrono::Duration::seconds(REPLICA_STALE_SECONDS);
    let live = db
        .fetch_replicas(
            Some(ReplicaKind::Worker),
            Some(ReplicaStatus::Live),
            stale_before,
        )
        .await?;
    Ok(live.iter().any(|replica| replica.replica_id == replica_id))
}

/// park an action node whose bound worker is unavailable: mark it waiting (once) with the node's
/// timeout armed, then re-arm a poll so it re-checks when the worker reconnects.
async fn park_for_target<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
) -> Result<(), SendableError> {
    if node_run.status != WorkflowStatus::Waiting {
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Waiting,
            None,
            None,
            None,
            None,
            Some("awaiting_local_worker".into()),
            None,
        )
        .await?;
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Waiting,
            Some(node.id.clone()),
            None,
            None,
        )
        .await?;
        arm_node_timeout(db, workflow_run.id, node).await?;
    }
    enqueue_target_poll(db, workflow_run.id, node).await
}

/// schedule the next re-check of a parked action node's bound worker.
async fn enqueue_target_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(LOCAL_TARGET_POLL_SECONDS);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "local_target_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

pub(super) struct ActionHandler;

impl<T: DatabaseImpl> NodeHandler<T> for ActionHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            if super::compute::is_inprocess_compute(ctx.node) {
                super::compute::process_compute_node(
                    ctx.db,
                    ctx.workflow,
                    ctx.workflow_run,
                    ctx.node,
                    ctx.node_runs,
                    ctx.nodes,
                )
                .await?;
            } else {
                process_action_node(
                    ctx.db,
                    ctx.workflow,
                    ctx.workflow_run,
                    ctx.node,
                    ctx.latest,
                    ctx.node_runs,
                )
                .await?;
            }
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
