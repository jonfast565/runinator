use std::future::Future;

use super::context::{is_reentry_stale, merge_parameters, runtime_context};
use super::handler::{NodeHandler, NodeHandlerContext};
use super::transitions::{
    arm_node_timeout, arm_node_timeout_or, retry_or_transition, time_out, timed_out,
    timed_out_since_created_or,
};
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
// how often a dispatched action rechecks that the worker executing it (its executor-claim holder,
// or a worker matching its pinned label selector/replica) is still live, so a worker dying mid-run
// fails the node promptly rather than waiting out (or never reaching) its `timeout_seconds`
// deadline.
const DISPATCH_LIVENESS_POLL_SECONDS: i64 = 15;
// fallback deadline for a node parked on an unavailable worker target when it declares no
// `timeout_seconds` of its own. the matching worker may never connect, and an indefinite park
// holds mutexes and blocks every run queued behind it, so the park must fail eventually.
pub(super) const TARGET_PARK_DEFAULT_TIMEOUT_SECONDS: i64 = 3600;

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

/// pure routing policy for a label-targeted action: dispatch to any worker carrying the required
/// labels when one is live, otherwise park until one connects. split from the db lookup for testing.
pub(super) fn target_for_labels(
    required_labels: &std::collections::BTreeMap<String, String>,
    worker_available: bool,
) -> TargetResolution {
    if worker_available {
        TargetResolution::Ready(ActionTarget::Labels {
            selector: required_labels.clone(),
        })
    } else {
        TargetResolution::Park
    }
}

/// whether a replica's advertised `attributes.labels` object is a superset of the required selector.
pub(super) fn replica_labels_match(
    attributes: &Value,
    required_labels: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some(labels) = attributes.get("labels").and_then(Value::as_object) else {
        return false;
    };
    required_labels
        .iter()
        .all(|(key, value)| labels.get(key).and_then(Value::as_str) == Some(value.as_str()))
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
            // node's timeout so a lost worker or dropped result cannot park the run forever. this
            // is a backstop for a missing/very long timeout_seconds; the liveness checks below are
            // what actually catch a worker dying mid-run in a timely fashion.
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
            // the executor claim names the worker actually running this action (any target,
            // including the general pool). a dead holder can never publish a result, and its fresh
            // claim makes every redelivery/retry get dropped as a duplicate — so release the claim
            // before failing, or the retry this schedules would be swallowed until the claim ages
            // past the timeout+grace staleness deadline.
            if let Some(holder) = node_run.current_executor_replica_id
                && !replica_is_live(db, holder).await?
            {
                db.release_workflow_node_run_executor(node_run.id, holder, Utc::now())
                    .await?;
                return time_out(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    "Worker executing this action disconnected",
                    node_runs,
                )
                .await;
            }
            // a pinned target (an explicit label selector, or the session-bound local provider)
            // has exactly one worker (or worker set) that can ever service it; if that worker just
            // went stale, no redelivery will ever land, so fail promptly instead of waiting out the
            // full timeout. this also covers a crash before the executor claim was recorded. the
            // claim (if any) is left alone here: its holder is live per the check above, so freeing
            // it could let a duplicate delivery run concurrently with the real execution.
            if dispatch_target_still_live(db, workflow_run, workflow, action).await? == Some(false)
            {
                return time_out(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    "Worker executing this action disconnected",
                    node_runs,
                )
                .await;
            }
            // keep watching every dispatched action (not just pinned targets): the executor-claim
            // check above is the only prompt dead-worker detection a general-pool dispatch has,
            // and it only runs when something drives this node.
            arm_dispatch_liveness_poll(db, workflow_run.id, node).await?;
            return Ok(());
        }
        // a node parked waiting for its bound desktop worker; honor the timeout, otherwise fall
        // through to re-resolve the target (the worker may have reconnected) reusing this run. a
        // parked run never touches `started_at` (only a `Running` transition sets it), so the
        // deadline is measured from `created_at` instead, with a fallback deadline when the node
        // declares no timeout of its own.
        if node_run.status == WorkflowStatus::Waiting
            && timed_out_since_created_or(node, node_run, TARGET_PARK_DEFAULT_TIMEOUT_SECONDS)
        {
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
                super::context::most_recently_finished_node_run(node_runs),
            )
            .await?
        }
    };
    // resolve routing before any observable dispatch: a session-bound action whose desktop worker is
    // not connected parks (and re-checks) instead of being published to a queue no one drains.
    let target = match resolve_action_target(db, workflow_run, workflow, action).await? {
        TargetResolution::Ready(target) => {
            tracing::info!(
                node_id = %node.id,
                action = %format!("{}.{}", action.provider, action.function),
                target = ?target,
                "action node dispatching to worker target"
            );
            target
        }
        TargetResolution::Park => {
            let required_labels = effective_required_labels(db, workflow, action).await?;
            tracing::warn!(
                node_id = %node.id,
                action = %format!("{}.{}", action.provider, action.function),
                required_labels = ?required_labels,
                "action node parking: no live worker matches its target; will fail on node timeout"
            );
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
    arm_node_timeout(db, workflow_run.id, node).await?;
    // the executing worker can die mid-run with no result ever arriving; re-check its liveness
    // (executor claim holder, or the pinned target's worker set) well before the possibly long,
    // or unset, node timeout would otherwise catch it.
    arm_dispatch_liveness_poll(db, workflow_run.id, node).await?;
    Ok(())
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
        // capture the dispatching trace so the worker's execution span joins this trace.
        trace_context: runinator_utilities::telemetry::current_trace_context(),
    }
}

/// decide which worker(s) may run this action. general-pool actions go to `Any`; the session-bound
/// local-files provider is pinned to the desktop replica that launched the run, and parks when that
/// replica is not currently connected so the action is never published into a queue no one drains.
async fn resolve_action_target<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    action: &WorkflowAction,
) -> Result<TargetResolution, SendableError> {
    let required_labels = effective_required_labels(db, workflow, action).await?;
    // an explicit label requirement takes precedence over provider-based routing: dispatch to a live
    // worker that carries the labels, otherwise park until one connects (the node timeout fails it).
    if !required_labels.is_empty() {
        let worker_available = live_worker_matches_labels(db, &required_labels).await?;
        tracing::debug!(
            required_labels = ?required_labels,
            worker_available,
            "resolving label-targeted action"
        );
        return Ok(target_for_labels(&required_labels, worker_available));
    }
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

/// the effective label selector for an action: its own `required_labels`, plus an `org=<slug>`
/// affinity label when the owning org has opted into dedicated workers (hybrid: shared pool by
/// default, dedicated opt-in). org-less or non-dedicated workflows keep running on the shared pool.
/// shared by dispatch-time routing and the post-dispatch liveness recheck so both apply identical
/// policy.
async fn effective_required_labels<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    action: &WorkflowAction,
) -> Result<std::collections::BTreeMap<String, String>, SendableError> {
    let mut required_labels = action.required_labels.clone();
    if let Some(org_id) = workflow.org_id {
        if let Some(slug) = org_dedicated_worker_slug(db, org_id).await? {
            required_labels.entry("org".to_string()).or_insert(slug);
        }
    }
    Ok(required_labels)
}

/// whether the worker(s) a dispatched action is pinned to are still live: `Some(true)`/`Some(false)`
/// for a pinned target (label selector or the session-bound local provider), `None` for a
/// general-pool dispatch, which has no single worker to go stale (the broker redelivers it instead).
async fn dispatch_target_still_live<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    action: &WorkflowAction,
) -> Result<Option<bool>, SendableError> {
    let required_labels = effective_required_labels(db, workflow, action).await?;
    if !required_labels.is_empty() {
        return Ok(Some(
            live_worker_matches_labels(db, &required_labels).await?,
        ));
    }
    if action.provider == LOCAL_PROVIDER {
        if let Some(replica_id) = workflow_run.trigger_actor_replica_id {
            return Ok(Some(replica_is_live(db, replica_id).await?));
        }
    }
    Ok(None)
}

/// schedule the next liveness recheck of a dispatched action's executing worker.
async fn arm_dispatch_liveness_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(DISPATCH_LIVENESS_POLL_SECONDS);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "dispatch_liveness_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// the org's slug when it has a dedicated worker allocation (`desired > 0`), else `None`. used to
/// add an `org=<slug>` routing label so a dedicated tenant's work lands on its own labeled workers.
async fn org_dedicated_worker_slug<T: DatabaseImpl>(
    db: &T,
    org_id: Uuid,
) -> Result<Option<String>, SendableError> {
    let groups = db.list_org_resource_groups(org_id).await?;
    if !has_dedicated_workers(&groups) {
        return Ok(None);
    }
    Ok(db.fetch_org(org_id).await?.map(|org| org.slug))
}

/// whether an org has a live dedicated worker allocation (`worker` kind with `desired > 0`).
pub(crate) fn has_dedicated_workers(
    groups: &[runinator_models::billing::OrgResourceGroup],
) -> bool {
    groups
        .iter()
        .any(|group| group.kind == ReplicaKind::Worker && group.desired > 0)
}

/// whether any live worker replica advertises labels that satisfy the action's required selector.
async fn live_worker_matches_labels<T: DatabaseImpl>(
    db: &T,
    required_labels: &std::collections::BTreeMap<String, String>,
) -> Result<bool, SendableError> {
    let stale_before = Utc::now() - chrono::Duration::seconds(REPLICA_STALE_SECONDS);
    let live = db
        .fetch_replicas(
            Some(ReplicaKind::Worker),
            Some(ReplicaStatus::Live),
            stale_before,
        )
        .await?;
    Ok(live
        .iter()
        .any(|replica| replica_labels_match(&replica.attributes, required_labels)))
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
        arm_node_timeout_or(
            db,
            workflow_run.id,
            node,
            TARGET_PARK_DEFAULT_TIMEOUT_SECONDS,
        )
        .await?;
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
