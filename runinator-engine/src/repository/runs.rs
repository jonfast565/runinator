use super::support;
use super::*;
use runinator_broker_core::IngressMessage;
use runinator_comm::WsIngressCommand;
use runinator_models::interrupt::{InterruptSource, PendingInterrupt};
use runinator_models::workflow_state::WorkflowExecutionState;
use uuid::Uuid;

use runinator_database::workflow_mutex::WorkflowMutexWake;

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
    let _ = super::console::settle_cell_for_run(db, ready_node.workflow_run_id).await;
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
            let _ = super::console::settle_cell_for_run(db, workflow_run_id).await;
            return Ok(Some(workflow_run_id));
        }
    };
    if disposition == crate::orchestration::ReadyNodeDisposition::KeepClaim {
        // not yet settled; return it to the queue so a later wake re-drives it.
        db.release_ready_node(ready_node_id, driver_id).await?;
        return Ok(Some(workflow_run_id));
    }
    db.complete_ready_node(ready_node_id, driver_id).await?;
    let _ = super::console::settle_cell_for_run(db, workflow_run_id).await;
    Ok(Some(workflow_run_id))
}

/// release every mutex owned by a run settled outside the reducer and wake each fifo successor.
pub async fn release_run_mutexes<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<(), SendableError> {
    let wakes = db
        .release_workflow_mutexes(workflow_run_id, Utc::now().timestamp())
        .await?;
    for wake in wakes {
        enqueue_mutex_wake(db, wake).await?;
    }
    Ok(())
}

async fn enqueue_mutex_wake<T: DatabaseImpl>(
    db: &T,
    wake: WorkflowMutexWake,
) -> Result<(), SendableError> {
    let mut event = NewOrchestrationEvent::new(
        wake.workflow_run_id,
        Some(wake.node_id.clone()),
        "mutex_released",
        runinator_models::json!({
            "node_id": wake.node_id,
            "workflow_node_run_id": wake.workflow_node_run_id,
        }),
    )
    .for_cursor(wake.cursor_id);
    event.workflow_node_run_id = Some(wake.workflow_node_run_id);
    db.enqueue_ready_node(event, wake.node_id, Utc::now())
        .await?;
    Ok(())
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
    release_run_mutexes(db, ready_node.workflow_run_id).await?;
    db.complete_ready_node(ready_node.id, driver_id).await?;
    Ok(())
}

const READY_NODE_DRIVE_LEASE_SECONDS: i64 = 60;

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
            let command =
                WsIngressCommand::drive(node.id, node.workflow_run_id, node.node_id, trace_id);
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
        let message = runinator_broker_core::WakeMessage {
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
    state: Option<WorkflowExecutionState>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_run_status(workflow_run_id, status, active_node_id, state, message)
        .await?;
    if status.is_terminal() {
        release_run_mutexes(db, workflow_run_id).await?;
    }
    Ok(TaskResponse {
        success: true,
        message: "Workflow run updated".into(),
    })
}

/// deliver a named signal to a run: find the latest node parked on that signal, stamp it
/// `Succeeded` with the payload, and wake the reducer so it follows the success edge.
/// how many times a losing state writer rebuilds its change before giving up. a conflict means the
/// reducer wrote first, and is resolved by re-reading rather than waiting.
const MAX_EVENT_DELIVERY_ATTEMPTS: usize = 8;

/// stamp an inbound event into the delivery slot a parked `event_source` node reads, then wake the
/// run so the reducer consumes it on its next drive.
///
/// the slot lives in the run state rather than on the node run because the node re-parks after each
/// event, and the state object is what survives that. delivering to a node that is not waiting is
/// reported back rather than silently dropped, so a misrouted webhook is visible.
pub async fn deliver_run_event<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    event: Value,
) -> Result<TaskResponse, SendableError> {
    let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let waiting = node_runs
        .iter()
        .any(|run| run.node_id == node_id && run.status == WorkflowStatus::Waiting);
    if !waiting {
        return Ok(TaskResponse {
            success: false,
            message: format!(
                "No event_source node '{node_id}' is waiting in run {workflow_run_id}"
            ),
        });
    }

    let mut delivered = false;
    for _ in 0..MAX_EVENT_DELIVERY_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(TaskResponse {
                success: false,
                message: format!("Workflow run {workflow_run_id} not found"),
            });
        };
        let mut state = run.execution_state.clone();
        state.deliver_event(&node_id, event.clone());
        if db
            .update_workflow_run_execution_state_cas(workflow_run_id, run.state_version, state)
            .await?
        {
            delivered = true;
            break;
        }
    }
    if !delivered {
        return Ok(TaskResponse {
            success: false,
            message: format!("Run {workflow_run_id} state kept changing; event not delivered"),
        });
    }

    support::enqueue_node_ready(
        db,
        workflow_run_id,
        node_id.clone(),
        "event_delivered",
        Utc::now(),
        runinator_models::json!({ "node_id": node_id }),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Event delivered to '{node_id}'"),
    })
}

/// does this run's workflow declare a handler for `source`?
///
/// asked before changing an existing behaviour on a run's behalf, so a workflow that never opted in
/// keeps the behaviour it had.
async fn declares_interrupt<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    source: InterruptSource,
) -> Result<bool, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(false);
    };
    let workflow = match run.workflow_snapshot {
        Some(snapshot) => snapshot,
        None => match db.fetch_workflow(run.workflow_id).await? {
            Some(workflow) => workflow,
            None => return Ok(false),
        },
    };
    Ok(runinator_workflows::interrupt_declarations_for(&workflow)
        .into_iter()
        .any(|declaration| declaration.enabled && declaration.source() == Some(source)))
}

/// ask a run to raise an interrupt on its next drive.
///
/// the request is recorded on the run rather than raised here: every rule about whether an interrupt
/// can be serviced lives in the reducer, and duplicating any of it in the web service is how the two
/// come to disagree. the reducer consumes the request on the drive that decides about it, so a
/// request nobody can service is dropped rather than left to fire at some arbitrary later point.
///
/// `cursor_id` names one thread of control; `None` lets whichever real thread drives next take it.
pub async fn request_run_interrupt<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    source: InterruptSource,
    payload: Value,
    cursor_id: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(TaskResponse {
            success: false,
            message: format!("Workflow run {workflow_run_id} not found"),
        });
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: false,
            message: format!("Workflow run {workflow_run_id} has already finished"),
        });
    }
    let request = PendingInterrupt::new(source, payload, cursor_id);

    // captured from the attempt that actually won the compare-and-swap, not the outer pre-loop
    // fetch: a concurrent drive can move the cursor between that fetch and a winning retry, and
    // deriving the wake target from the stale copy could wake the wrong node or miss it entirely.
    let mut recorded: Option<runinator_models::workflows::WorkflowRun> = None;
    for _ in 0..MAX_EVENT_DELIVERY_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(TaskResponse {
                success: false,
                message: format!("Workflow run {workflow_run_id} not found"),
            });
        };
        let mut state = run.execution_state.clone();
        // replayable: the request carries its own id, so a losing writer re-adds the same one on top
        // of whatever won rather than accumulating duplicates.
        state.take_pending_interrupt(request.id);
        state.pending_interrupts.push(request.clone());
        if db
            .update_workflow_run_execution_state_cas(workflow_run_id, run.state_version, state)
            .await?
        {
            recorded = Some(run);
            break;
        }
    }
    let Some(run) = recorded else {
        return Ok(TaskResponse {
            success: false,
            message: format!("Run {workflow_run_id} state kept changing; interrupt not requested"),
        });
    };

    // wake the thread so the request is looked at now rather than whenever the run next happens to
    // move. an untargeted request rides the run's mirrored position, which is the primary cursor.
    let node_id = match cursor_id.and_then(|id| run.execution_state.cursor(id).cloned()) {
        Some(cursor) => Some(cursor.node_id().to_string()),
        None => run.active_node_id.clone(),
    };
    if let Some(node_id) = node_id {
        match cursor_id {
            Some(cursor_id) => {
                support::enqueue_node_ready_for_cursor(
                    db,
                    workflow_run_id,
                    cursor_id,
                    node_id,
                    "interrupt_requested",
                    Utc::now(),
                )
                .await?
            }
            None => {
                support::enqueue_node_ready(
                    db,
                    workflow_run_id,
                    node_id,
                    "interrupt_requested",
                    Utc::now(),
                    runinator_models::json!({ "source": source.as_str() }),
                )
                .await?
            }
        }
    }
    Ok(TaskResponse {
        success: true,
        message: format!("Interrupt '{source}' requested for run {workflow_run_id}"),
    })
}

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
        // nothing is parked on this signal. a run that declared an `orphan_signal` handler wants to
        // hear about that; every other run keeps the old behaviour of reporting it back, so a
        // misrouted webhook stays visible rather than being quietly absorbed.
        if declares_interrupt(db, workflow_run_id, InterruptSource::OrphanSignal).await? {
            return request_run_interrupt(
                db,
                workflow_run_id,
                InterruptSource::OrphanSignal,
                runinator_models::json!({ "signal": name, "payload": payload }),
                None,
            )
            .await;
        }
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
