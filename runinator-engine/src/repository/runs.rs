use super::support;
use super::*;
use runinator_broker_core::IngressMessage;
use runinator_comm::WsIngressCommand;
use runinator_models::interrupt::{InterruptSource, PendingInterrupt};
use runinator_models::workflow_state::WorkflowExecutionState;
use uuid::Uuid;

use runinator_database::{roles::NewWorkflowVmRun, workflow_mutex::WorkflowMutexWake};

pub async fn delete_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow_run(workflow_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run deleted".into(),
    })
}

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
    create_workflow_vm_run(
        db,
        workflow_id,
        workflow_snapshot,
        parameters,
        state,
        trimmed,
        provenance,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_workflow_vm_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
    workflow_snapshot: WorkflowDefinition,
    parameters: Value,
    state: Value,
    name: Option<String>,
    provenance: runinator_models::replicas::WorkflowRunProvenance,
    pipeline_run_id: Option<Uuid>,
    start_node_id: Option<&str>,
) -> Result<WorkflowRun, SendableError> {
    let module = runinator_workflows::compile_workflow_module(&workflow_snapshot)
        .map_err(|error| -> SendableError { Box::new(error) })?;
    let instruction_pointer = if let Some(node_id) = start_node_id {
        module
            .source_map
            .iter()
            .find(|entry| entry.node_id == node_id)
            .map(|entry| entry.instruction_start)
            .ok_or_else(|| crate::errors::REPLAY_MISSING_STEP.error(node_id))?
    } else {
        0
    };
    db.create_workflow_vm_run(NewWorkflowVmRun {
        workflow_id,
        workflow_snapshot,
        parameters,
        state,
        name,
        provenance,
        pipeline_run_id,
        pipeline_member_attempt_id: None,
        module,
        instruction_pointer,
    })
    .await
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

/// Fetch a VM-backed workflow run. Continuations, effects, and journal entries are read through
/// their dedicated resources; node-run history is intentionally not reconstructed here.
pub async fn fetch_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Option<WorkflowRun>, SendableError> {
    db.fetch_workflow_run(workflow_run_id).await
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
    if let Some(module) = db.fetch_workflow_module(workflow_run_id).await? {
        let effects = db.fetch_workflow_effects(workflow_run_id).await?;
        for effect in effects.into_iter().rev() {
            if effect.status.is_terminal()
                || !matches!(
                    effect.request,
                    runinator_models::workflow_vm::WorkflowEffectRequest::EventWait { .. }
                )
            {
                continue;
            }
            let Some(continuation) = db
                .fetch_workflow_continuation(effect.continuation_id)
                .await?
            else {
                continue;
            };
            let effect_ip = continuation.instruction_pointer.saturating_sub(1);
            if module
                .graph_location(effect_ip)
                .map(|location| location.node_id.as_str())
                != Some(node_id.as_str())
            {
                continue;
            }
            let applied = db
                .settle_workflow_effect(
                    effect.id,
                    effect.attempt,
                    runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
                    Some(event),
                    Some("event_received".into()),
                    Utc::now(),
                )
                .await?;
            return Ok(TaskResponse {
                success: applied,
                message: if applied {
                    format!("Event delivered to '{node_id}'")
                } else {
                    format!("Event for '{node_id}' was stale")
                },
            });
        }
        return Ok(TaskResponse {
            success: false,
            message: format!(
                "No event effect for node '{node_id}' is waiting in run {workflow_run_id}"
            ),
        });
    }
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
    if db.fetch_workflow_module(workflow_run_id).await?.is_some() {
        let effects = db.fetch_workflow_effects(workflow_run_id).await?;
        let target = effects.into_iter().rev().find(|effect| {
            !effect.status.is_terminal()
                && matches!(&effect.request,
                    runinator_models::workflow_vm::WorkflowEffectRequest::Signal { key, .. }
                    if key == &name)
        });
        let Some(effect) = target else {
            return Ok(TaskResponse {
                success: false,
                message: format!(
                    "No effect is waiting for signal '{name}' in run {workflow_run_id}"
                ),
            });
        };
        let applied = db
            .settle_workflow_effect(
                effect.id,
                effect.attempt,
                runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
                Some(runinator_models::json!({ "signal": name, "payload": payload })),
                Some("signal_received".into()),
                Utc::now(),
            )
            .await?;
        return Ok(TaskResponse {
            success: applied,
            message: if applied {
                format!("Signal '{name}' delivered")
            } else {
                format!("Signal '{name}' was stale")
            },
        });
    }
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
