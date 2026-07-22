use super::support;
use super::*;
use runinator_broker::IngressMessage;
use runinator_comm::WsIngressCommand;
use uuid::Uuid;

pub async fn create_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
    parameters: Value,
    debug: bool,
    name: Option<String>,
    provenance: runinator_models::replicas::WorkflowRunProvenance,
) -> Result<WorkflowRun, SendableError> {
    let workflow_snapshot = support::fetch_workflow_snapshot(db, workflow_id).await?;
    let state = if debug {
        runinator_models::json!({
            "control": { "pause_requested": false },
            "debug": {
                "enabled": true,
                "paused": false,
                "step_requested": false,
                "mode": "breakpoints",
                "breakpoints": [],
                "one_shot_breakpoint": null
            }
        })
    } else {
        runinator_models::json!({ "control": { "pause_requested": false } })
    };
    let trimmed = support::normalized_run_name(name);
    let run = db
        .create_workflow_run(
            workflow_id,
            workflow_snapshot,
            parameters,
            state,
            trimmed,
            provenance,
        )
        .await?;
    support::enqueue_start_ready_node(db, &run).await?;
    Ok(run)
}

pub async fn claim_ready_nodes<T: DatabaseImpl>(
    db: &T,
    scheduler_id: String,
    lease_until: chrono::DateTime<Utc>,
    limit: i64,
) -> Result<Vec<ReadyNodeRecord>, SendableError> {
    db.claim_ready_nodes(scheduler_id, Utc::now(), lease_until, limit)
        .await
}

pub async fn complete_ready_node<T: DatabaseImpl>(
    db: &T,
    ready_node_id: Uuid,
    scheduler_id: String,
    next_ready: Option<(Uuid, String, chrono::DateTime<Utc>)>,
) -> Result<TaskResponse, SendableError> {
    let Some(ready_node) = db.fetch_ready_node(ready_node_id).await? else {
        return Err(runinator_reducer::errors::READY_NODE_NOT_FOUND.error(ready_node_id));
    };
    if ready_node.claimed_by.as_deref() != Some(scheduler_id.as_str()) {
        return Err(runinator_reducer::errors::READY_NODE_NOT_CLAIMED.error(ready_node_id));
    }
    let disposition = crate::orchestration::process_ready_node(db, &ready_node).await?;
    if disposition == crate::orchestration::ReadyNodeDisposition::KeepClaim {
        return Ok(TaskResponse {
            success: true,
            message: "Ready node remains claimed until it is due".into(),
        });
    }
    if !db.complete_ready_node(ready_node_id, scheduler_id).await? {
        return Err(runinator_reducer::errors::READY_NODE_NOT_CLAIMED.error(ready_node_id));
    }
    if let Some((workflow_run_id, node_id, ready_at)) = next_ready {
        support::enqueue_node_ready(
            db,
            workflow_run_id,
            node_id.clone(),
            "node_waiting",
            ready_at,
            runinator_models::json!({ "node_id": node_id }),
        )
        .await?;
    }
    Ok(TaskResponse {
        success: true,
        message: "Ready node processed".into(),
    })
}

/// drive a single ready node by id over the broker ingress path. the web service claims the row
/// itself (the waker has no database), runs the reducer, then completes or releases it. returns the
/// workflow run id on success so the caller can emit a ui event. a `None` means the row was already
/// completed or claimed elsewhere and there was nothing to do.
pub async fn drive_ready_node<T: DatabaseImpl>(
    db: &T,
    ready_node_id: Uuid,
    driver_id: String,
) -> Result<Option<Uuid>, SendableError> {
    let now = Utc::now();
    let lease_until = now + Duration::seconds(READY_NODE_DRIVE_LEASE_SECONDS);
    let Some(ready_node) = db
        .claim_ready_node(ready_node_id, driver_id.clone(), now, lease_until)
        .await?
    else {
        return Ok(None);
    };
    let workflow_run_id = ready_node.workflow_run_id;
    let disposition = match crate::orchestration::process_ready_node(db, &ready_node).await {
        Ok(disposition) => disposition,
        Err(err) => {
            // a reducer hard-error would otherwise leave the row claimed and get re-driven every
            // lease period (a poison pill). fail the run and settle the row so it stops looping.
            fail_driven_ready_node(db, &ready_node, driver_id, err.as_ref()).await?;
            return Ok(Some(workflow_run_id));
        }
    };
    if disposition == crate::orchestration::ReadyNodeDisposition::KeepClaim {
        // not yet settled; return it to the queue so a later wake re-drives it.
        db.release_ready_node(ready_node_id, driver_id).await?;
        return Ok(Some(workflow_run_id));
    }
    db.complete_ready_node(ready_node_id, driver_id).await?;
    Ok(Some(workflow_run_id))
}

/// settle a ready node whose reducer hard-errored: mark the run failed and complete the row so the
/// drive loop does not re-claim and re-run it every lease period.
async fn fail_driven_ready_node<T: DatabaseImpl>(
    db: &T,
    ready_node: &ReadyNodeRecord,
    driver_id: String,
    err: &(dyn std::error::Error + Send + Sync + 'static),
) -> Result<(), SendableError> {
    log::error!(
        "Reducer failed for ready node {} (workflow run {}, node {}) [{}]: {}",
        ready_node.id,
        ready_node.workflow_run_id,
        ready_node.node_id,
        runinator_models::errors::error_code_or_unknown(err),
        err
    );
    db.update_workflow_run_status(
        ready_node.workflow_run_id,
        WorkflowStatus::Failed,
        Some(ready_node.node_id.clone()),
        None,
        Some(format!(
            "Reducer error driving node {}: {}",
            ready_node.node_id, err
        )),
    )
    .await?;
    db.complete_ready_node(ready_node.id, driver_id).await?;
    Ok(())
}

const READY_NODE_DRIVE_LEASE_SECONDS: i64 = 60;

/// drain durable action-dispatch intents and publish them to the broker action channel. moved into
/// the web service (which owns the database and the reducer) so the waker no longer relays them.
pub async fn publish_pending_action_dispatches<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    publisher_id: &str,
    lease_seconds: i64,
    limit: i64,
) -> Result<(), SendableError> {
    let now = Utc::now();
    let lease_until = now + Duration::seconds(lease_seconds);
    let dispatches = db
        .claim_pending_action_dispatches(publisher_id.to_string(), now, lease_until, limit)
        .await?;
    for dispatch in dispatches {
        let dispatch_id = dispatch.id;
        let message = BrokerMessage {
            command: dispatch.command,
            dedupe_key: Some(dispatch.dedupe_key),
            enqueued_at: Utc::now(),
        };
        match broker.publish(message).await {
            Ok(()) | Err(BrokerError::Duplicate(_)) => {
                db.mark_action_dispatch_published(dispatch_id).await?;
            }
            Err(err) => {
                db.mark_action_dispatch_failed(dispatch_id, err.to_string())
                    .await?;
            }
        }
    }
    Ok(())
}

// how long a wake announcement stays leased in the database. a pending ready node is announced at
// most once per window, so backends without broker-side dedupe (rabbitmq, kafka) do not accumulate
// duplicate wakes; a wake lost in flight is re-announced once the lease lapses after its due time.
const WAKE_ANNOUNCE_LEASE_SECONDS: i64 = 30;

/// announce pending ready nodes for drive. due nodes (`ready_at <= now`) publish a Drive straight
/// onto the ingress channel so queue→running (and node→node) is not gated on a waker broker hop;
/// future-dated nodes still publish a Wake for the waker to sleep until due. doubles as the durable
/// backstop via the announce lease; the broker dedupes wakes/drives already in flight.
pub async fn publish_pending_wakes<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    limit: i64,
) -> Result<(), SendableError> {
    let now = Utc::now();
    let pending = db
        .claim_ready_nodes_for_announce(now, WAKE_ANNOUNCE_LEASE_SECONDS, limit)
        .await?;
    for node in pending {
        let trace_id = Uuid::now_v7();
        if node.ready_at <= now {
            // already due: skip wake→waker→ingress and drive immediately.
            let command = WsIngressCommand::drive(
                node.id,
                node.workflow_run_id,
                node.node_id,
                trace_id,
            );
            let message = IngressMessage {
                command,
                dedupe_key: None,
                enqueued_at: Utc::now(),
            };
            match broker.publish_ingress(message).await {
                Ok(()) | Err(BrokerError::Duplicate(_)) => {}
                Err(err) => {
                    log::warn!(
                        "Failed to publish drive for due ready node {}: {}",
                        node.id,
                        err
                    );
                }
            }
            continue;
        }

        let command = runinator_comm::WakeCommand::new(
            node.id,
            node.workflow_run_id,
            node.node_id,
            node.ready_at,
            node.source_event_id,
            trace_id,
        );
        let message = runinator_broker::WakeMessage {
            command,
            dedupe_key: None,
            enqueued_at: Utc::now(),
        };
        match broker.publish_wake(message).await {
            Ok(()) | Err(BrokerError::Duplicate(_)) => {}
            Err(err) => {
                log::warn!("Failed to publish wake for ready node {}: {}", node.id, err);
            }
        }
    }
    Ok(())
}

/// safety backstop: settle uncompleted ready nodes whose run is already terminal, in bounded
/// batches. the reducer settles these inline on the terminal transition; this catches any orphaned
/// by a crash mid-transition so the wake publisher stops rescanning dead runs. returns rows settled.
pub async fn settle_terminal_run_ready_nodes<T: DatabaseImpl>(
    db: &T,
    limit: i64,
) -> Result<u64, SendableError> {
    db.settle_terminal_run_ready_nodes(limit).await
}

pub async fn fetch_workflow_runs_by_status<T: DatabaseImpl>(
    db: &T,
    status: WorkflowStatus,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_by_status(status).await
}

pub async fn claim_workflow_runs_for_scheduler<T: DatabaseImpl>(
    db: &T,
    scheduler_id: String,
    statuses: Vec<WorkflowStatus>,
    lease_until: chrono::DateTime<Utc>,
    limit: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.claim_workflow_runs_for_scheduler(scheduler_id, statuses, Utc::now(), lease_until, limit)
        .await
}

pub async fn renew_workflow_run_claim<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    scheduler_id: String,
    lease_until: chrono::DateTime<Utc>,
) -> Result<bool, SendableError> {
    db.renew_workflow_run_claim(workflow_run_id, scheduler_id, lease_until)
        .await
}

pub async fn release_workflow_run_claim<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    scheduler_id: String,
) -> Result<(), SendableError> {
    db.release_workflow_run_claim(workflow_run_id, scheduler_id)
        .await
}

pub async fn fetch_recent_workflow_runs<T: DatabaseImpl>(
    db: &T,
    limit: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_recent_workflow_runs(limit).await
}

pub async fn fetch_workflow_runs_for_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_for_workflow(workflow_id).await
}

pub async fn fetch_workflow_runs_by_name<T: DatabaseImpl>(
    db: &T,
    name: String,
    open_only: bool,
) -> Result<Vec<WorkflowRun>, SendableError> {
    let Some(name) = support::normalized_run_name(Some(name)) else {
        return Ok(Vec::new());
    };
    db.fetch_workflow_runs_by_name(name, open_only).await
}

pub async fn update_workflow_run_status<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    status: WorkflowStatus,
    active_node_id: Option<String>,
    state: Option<Value>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_run_status(workflow_run_id, status, active_node_id, state, message)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run updated".into(),
    })
}

/// deliver a named signal to a run: find the latest node parked on that signal, stamp it
/// `Succeeded` with the payload, and wake the reducer so it follows the success edge.
pub async fn deliver_signal<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    name: String,
    payload: Value,
) -> Result<TaskResponse, SendableError> {
    let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let target = node_runs
        .iter()
        .filter(|run| run.status == WorkflowStatus::Waiting)
        .filter(|run| {
            serde_json::from_value::<runinator_models::workflow_state::SignalState>(
                run.state.clone().into(),
            )
            .map(|state| state.name == name)
            .unwrap_or(false)
        })
        .max_by_key(|run| run.created_at);
    let Some(node_run) = target else {
        return Ok(TaskResponse {
            success: false,
            message: format!("No node is waiting for signal '{name}' in run {workflow_run_id}"),
        });
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        Some(runinator_models::json!({ "signal": name, "payload": payload })),
        None,
        Some("signal_received".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        Some(node_run.node_id.clone()),
        None,
        None,
    )
    .await?;
    support::enqueue_node_ready(
        db,
        workflow_run_id,
        node_run.node_id.clone(),
        "signal_received",
        Utc::now(),
        runinator_models::json!({ "signal": name }),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Signal '{name}' delivered"),
    })
}

/// route an inbound signal to a parked node by `(name, correlation_key)` across every run, so an
/// external webhook (github/jira/ci) can resolve the right run without knowing its id. resolves the
/// most recently parked match the same way as `deliver_signal`.
pub async fn deliver_signal_by_correlation<T: DatabaseImpl>(
    db: &T,
    name: String,
    correlation_key: String,
    payload: Value,
) -> Result<TaskResponse, SendableError> {
    let waiting = db
        .fetch_workflow_node_runs_by_status(WorkflowStatus::Waiting)
        .await?;
    let target = waiting
        .iter()
        .filter(|run| {
            serde_json::from_value::<runinator_models::workflow_state::SignalState>(
                run.state.clone().into(),
            )
            .map(|state| {
                state.name == name
                    && state.correlation_key.as_deref() == Some(correlation_key.as_str())
            })
            .unwrap_or(false)
        })
        .max_by_key(|run| run.created_at);
    let Some(node_run) = target else {
        return Ok(TaskResponse {
            success: false,
            message: format!(
                "No node is waiting for signal '{name}' with correlation key '{correlation_key}'"
            ),
        });
    };
    let workflow_run_id = node_run.workflow_run_id;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        Some(runinator_models::json!({
            "signal": name,
            "correlation_key": correlation_key,
            "payload": payload,
        })),
        None,
        Some("signal_received".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        Some(node_run.node_id.clone()),
        None,
        None,
    )
    .await?;
    support::enqueue_node_ready(
        db,
        workflow_run_id,
        node_run.node_id.clone(),
        "signal_received",
        Utc::now(),
        runinator_models::json!({ "signal": name, "correlation_key": correlation_key }),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Signal '{name}' delivered to run {workflow_run_id}"),
    })
}

pub async fn set_workflow_run_name<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    name: Option<String>,
) -> Result<TaskResponse, SendableError> {
    let trimmed = support::normalized_run_name(name);
    db.set_workflow_run_name(workflow_run_id, trimmed).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run renamed".into(),
    })
}
