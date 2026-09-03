use super::support;
use super::*;
use runinator_models::interrupt::InterruptSource;
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::{auth::ResourceType, settings::SettingBinding};
use uuid::Uuid;

use runinator_store::roles::NewWorkflowVmRun;

pub async fn delete_workflow_run<T: RunStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow_run(workflow_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run deleted".into(),
    })
}

pub async fn create_workflow_run<T: RuntimeStore + WorkflowVmStore>(
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
                "pause_on_failure": false,
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
pub(crate) async fn create_workflow_vm_run<T: RuntimeStore + WorkflowVmStore>(
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
    // Configuration is a run input, just like parameters: retain one resolved snapshot so an
    // in-flight run neither loses its settings nor changes behaviour after an edit.
    let config = runinator_runtime::config::config_tree_for_workflow(db, &workflow_snapshot).await;
    db.create_workflow_vm_run(NewWorkflowVmRun {
        workflow_id,
        workflow_snapshot,
        parameters,
        config,
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

pub async fn validate_workflow_dependency_access<
    T: RuntimeStore + runinator_store::roles::AuthStore + runinator_store::roles::RbacStore,
>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<(), SendableError> {
    let Some(workflow_id) = workflow.id else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workflow has no durable identity",
        )));
    };
    for (dependency_type, dependency_id) in workflow_dependency_refs(workflow) {
        if !runinator_store::resource_access::resource_can_consume(
            db,
            ResourceType::Workflow,
            workflow_id,
            dependency_type,
            dependency_id,
        )
        .await?
        {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "workflow {workflow_id} is not permitted to use {} {dependency_id}",
                    dependency_type.as_str()
                ),
            )));
        }
    }
    Ok(())
}

pub fn workflow_dependency_refs(workflow: &WorkflowDefinition) -> Vec<(ResourceType, Uuid)> {
    let settings = workflow
        .definition
        .metadata
        .pointer("/artifact_refs/settings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| serde_json::from_value::<SettingBinding>(value.clone().into()).ok())
        .filter(|binding| !binding.reference.id.is_nil())
        .map(|binding| (ResourceType::Setting, binding.reference.id));
    let profiles = workflow
        .definition
        .nodes
        .iter()
        .flat_map(|node| node.action.iter().chain(node.compensation.iter()))
        .filter_map(|action| action.execution_profile.as_ref())
        .filter(|binding| !binding.id().is_nil())
        .map(|binding| (ResourceType::ExecutionProfile, binding.id()));
    settings.chain(profiles).collect()
}

pub async fn fetch_workflow_runs_by_status<T: RunStore>(
    db: &T,
    status: WorkflowStatus,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_by_status(status).await
}

pub async fn claim_workflow_runs_for_scheduler<T: RunStore>(
    db: &T,
    scheduler_id: String,
    statuses: Vec<WorkflowStatus>,
    lease_until: chrono::DateTime<Utc>,
    limit: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.claim_workflow_runs_for_scheduler(scheduler_id, statuses, Utc::now(), lease_until, limit)
        .await
}

pub async fn renew_workflow_run_claim<T: RunStore>(
    db: &T,
    workflow_run_id: Uuid,
    scheduler_id: String,
    lease_until: chrono::DateTime<Utc>,
) -> Result<bool, SendableError> {
    db.renew_workflow_run_claim(workflow_run_id, scheduler_id, lease_until)
        .await
}

pub async fn release_workflow_run_claim<T: RunStore>(
    db: &T,
    workflow_run_id: Uuid,
    scheduler_id: String,
) -> Result<(), SendableError> {
    db.release_workflow_run_claim(workflow_run_id, scheduler_id)
        .await
}

pub async fn fetch_recent_workflow_runs<T: RunStore>(
    db: &T,
    limit: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_recent_workflow_runs(limit).await
}

pub async fn fetch_workflow_runs_for_workflow<T: RuntimeStore>(
    db: &T,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_for_workflow(workflow_id).await
}

/// Fetch a VM-backed workflow run. Continuations, effects, and journal entries are read through
/// their dedicated resources; node-run history is intentionally not reconstructed here.
pub async fn fetch_workflow_run<T: RuntimeStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Option<WorkflowRun>, SendableError> {
    db.fetch_workflow_run(workflow_run_id).await
}

pub async fn fetch_workflow_runs_by_name<T: RuntimeStore>(
    db: &T,
    name: String,
    open_only: bool,
) -> Result<Vec<WorkflowRun>, SendableError> {
    let Some(name) = support::normalized_run_name(Some(name)) else {
        return Ok(Vec::new());
    };
    db.fetch_workflow_runs_by_name(name, open_only).await
}

pub async fn update_workflow_run_status<T: RuntimeStore>(
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

/// stamp an inbound event into the delivery slot a parked `event_source` node reads, then wake the
/// run so the VM consumes it on its next drive.
///
/// the slot lives in the run state rather than on the node run because the node re-parks after each
/// event, and the state object is what survives that. delivering to a node that is not waiting is
/// reported back rather than silently dropped, so a misrouted webhook is visible.
pub async fn deliver_run_event<T: WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    event: Value,
) -> Result<TaskResponse, SendableError> {
    let Some(module) = db.fetch_workflow_module(workflow_run_id).await? else {
        return Ok(TaskResponse {
            success: false,
            message: format!("Workflow run {workflow_run_id} has no VM module"),
        });
    };
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
    Ok(TaskResponse {
        success: false,
        message: format!(
            "No event effect for node '{node_id}' is waiting in run {workflow_run_id}"
        ),
    })
}

/// does this run's workflow declare a handler for `source`?
///
/// asked before changing an existing behaviour on a run's behalf, so a workflow that never opted in
/// keeps the behaviour it had.
async fn declares_interrupt<T: RuntimeStore>(
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
/// Record an out-of-band interrupt request against one thread of a run.
///
/// Nothing about serviceability is decided here. The request is stamped on the target continuation
/// and the VM raises or refuses it at that thread's next safe point, which is the only place the
/// fail-open rules live; a second copy of them in the web service is how the two come to disagree.
/// The request is consumed by the drive that decides about it, so one nobody can service is dropped
/// rather than left to fire at some arbitrary later point.
///
/// `continuation_id` names one thread of control; `None` targets the run's oldest live one.
pub async fn request_run_interrupt<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    source: InterruptSource,
    payload: Value,
    continuation_id: Option<Uuid>,
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
    let pending = runinator_models::workflow_vm::WorkflowPendingInterrupt {
        id: Uuid::now_v7(),
        source,
        payload,
    };
    match db
        .request_workflow_interrupt(workflow_run_id, continuation_id, pending)
        .await?
    {
        Some(continuation_id) => Ok(TaskResponse {
            success: true,
            message: format!(
                "Interrupt '{source}' requested for run {workflow_run_id} on continuation {continuation_id}"
            ),
        }),
        None => Ok(TaskResponse {
            success: false,
            message: format!("Workflow run {workflow_run_id} has no live thread to interrupt"),
        }),
    }
}

pub async fn deliver_signal<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    name: String,
    payload: Value,
) -> Result<TaskResponse, SendableError> {
    if db.fetch_workflow_module(workflow_run_id).await?.is_none() {
        return Ok(TaskResponse {
            success: false,
            message: format!("Workflow run {workflow_run_id} has no VM module"),
        });
    }
    let effects = db.fetch_workflow_effects(workflow_run_id).await?;
    let target = effects.into_iter().rev().find(|effect| {
        !effect.status.is_terminal()
            && matches!(&effect.request,
                runinator_models::workflow_vm::WorkflowEffectRequest::Signal { key, .. }
                if key == &name)
    });
    let Some(effect) = target else {
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
            message: format!("No effect is waiting for signal '{name}' in run {workflow_run_id}"),
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
    Ok(TaskResponse {
        success: applied,
        message: if applied {
            format!("Signal '{name}' delivered")
        } else {
            format!("Signal '{name}' was stale")
        },
    })
}

pub async fn set_workflow_run_name<T: RuntimeStore>(
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
