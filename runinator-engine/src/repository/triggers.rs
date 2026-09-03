use super::support;
use super::*;
use runinator_store::roles::ScheduledWorkflowVm;
use uuid::Uuid;

pub async fn upsert_workflow_trigger<T: ScheduleStore>(
    db: &T,
    trigger: &WorkflowTrigger,
) -> Result<WorkflowTrigger, SendableError> {
    validate_trigger_schedules(&trigger.configuration)?;
    db.upsert_workflow_trigger(trigger).await
}

pub(crate) fn validate_trigger_schedules(configuration: &Value) -> Result<(), SendableError> {
    if let Some(value) = configuration.get("schedule") {
        let schedule: runinator_models::schedules::ScheduleSpec =
            serde_json::from_value(value.clone().into())?;
        runinator_scheduling::validate(&schedule)
            .map_err(|error| crate::errors::FREEZE_WINDOW_INVALID.error(error))?;
        if schedule.duration_seconds != 0 {
            return Err(crate::errors::FREEZE_WINDOW_INVALID
                .error("a firing schedule must have zero duration"));
        }
    }
    if let Some(value) = configuration.get("exclusions") {
        let exclusions: Vec<runinator_models::schedules::ScheduleSpec> =
            serde_json::from_value(value.clone().into())?;
        for exclusion in exclusions {
            runinator_scheduling::validate(&exclusion)
                .map_err(|error| crate::errors::FREEZE_WINDOW_INVALID.error(error))?;
            if exclusion.duration_seconds <= 0 {
                return Err(crate::errors::FREEZE_WINDOW_INVALID
                    .error("a schedule exclusion needs a positive duration"));
            }
        }
    }
    Ok(())
}

pub async fn fetch_workflow_triggers<T: ScheduleStore + RuntimeStore>(
    db: &T,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_triggers(workflow_id).await
}

pub async fn fetch_workflow_trigger<T: ScheduleStore>(
    db: &T,
    trigger_id: Uuid,
) -> Result<Option<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_trigger(trigger_id).await
}

pub async fn fetch_due_workflow_triggers<T: ScheduleStore>(
    db: &T,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_due_workflow_triggers(Utc::now()).await
}

pub async fn claim_due_workflow_trigger_firings<T: ScheduleStore + RuntimeStore>(
    db: &T,
    scheduler_id: String,
    limit: i64,
) -> Result<TriggerFiringBatch<WorkflowRun>, SendableError> {
    db.refresh_freeze_windows(Utc::now()).await?;
    let mut modules = std::collections::HashMap::new();
    for trigger in db.fetch_due_workflow_triggers(Utc::now()).await? {
        if modules.contains_key(&trigger.workflow_id) {
            continue;
        }
        let snapshot = support::fetch_workflow_snapshot(db, trigger.workflow_id).await?;
        let module = runinator_workflows::compile_workflow_module(&snapshot)
            .map_err(|error| -> SendableError { Box::new(error) })?;
        modules.insert(
            trigger.workflow_id,
            ScheduledWorkflowVm { snapshot, module },
        );
    }
    db.claim_due_workflow_trigger_firings(scheduler_id, Utc::now(), limit, modules)
        .await
}

/// replay a cron trigger's missed slots across a past range. slots the loop already fired keep
/// their original run, so a backfill is safe to re-issue over an overlapping range.
pub async fn backfill_workflow_trigger<T: ScheduleStore + RuntimeStore>(
    db: &T,
    trigger_id: Uuid,
    request: &BackfillRequest,
) -> Result<(BackfillResponse, Vec<WorkflowRun>), SendableError> {
    let trigger = db
        .fetch_workflow_trigger(trigger_id)
        .await?
        .ok_or_else(|| runinator_runtime::errors::WORKFLOW_TRIGGER_NOT_FOUND.error(trigger_id))?;
    let snapshot = support::fetch_workflow_snapshot(db, trigger.workflow_id).await?;
    let module = runinator_workflows::compile_workflow_module(&snapshot)
        .map_err(|error| -> SendableError { Box::new(error) })?;
    db.backfill_workflow_trigger(
        trigger_id,
        request,
        ScheduledWorkflowVm { snapshot, module },
    )
    .await
}

pub async fn fetch_freeze_windows<T: ScheduleStore>(
    db: &T,
    org_id: Option<Uuid>,
) -> Result<Vec<FreezeWindow>, SendableError> {
    db.fetch_freeze_windows(org_id).await
}

pub async fn fetch_freeze_window<T: ScheduleStore>(
    db: &T,
    window_id: Uuid,
) -> Result<Option<FreezeWindow>, SendableError> {
    db.fetch_freeze_window(window_id).await
}

pub async fn fetch_active_freeze_windows<T: ScheduleStore>(
    db: &T,
) -> Result<Vec<FreezeWindow>, SendableError> {
    db.refresh_freeze_windows(Utc::now()).await?;
    db.fetch_active_freeze_windows(Utc::now()).await
}

pub async fn create_freeze_window<T: ScheduleStore>(
    db: &T,
    window: &NewFreezeWindow,
) -> Result<FreezeWindow, SendableError> {
    validate_freeze_window(window)?;
    db.create_freeze_window(window).await
}

pub async fn update_freeze_window<T: ScheduleStore>(
    db: &T,
    window_id: Uuid,
    window: &NewFreezeWindow,
) -> Result<Option<FreezeWindow>, SendableError> {
    validate_freeze_window(window)?;
    db.update_freeze_window(window_id, window).await
}

pub async fn delete_freeze_window<T: ScheduleStore>(
    db: &T,
    window_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    if !db.delete_freeze_window(window_id).await? {
        return Err(crate::errors::FREEZE_WINDOW_NOT_FOUND.error(window_id));
    }
    Ok(TaskResponse {
        success: true,
        message: "Freeze window deleted".into(),
    })
}

/// reject windows that would store fine and then never freeze anything. an inverted range is the
/// likely shape of a mistyped date, and silently accepting it means discovering during the change
/// freeze that nothing was frozen.
fn validate_freeze_window(window: &NewFreezeWindow) -> Result<(), SendableError> {
    if window.name.trim().is_empty() {
        return Err(crate::errors::FREEZE_WINDOW_INVALID.error("name must not be empty"));
    }
    if window.ends_at <= window.starts_at {
        return Err(crate::errors::FREEZE_WINDOW_INVALID.error("ends_at must be after starts_at"));
    }
    if let Some(schedule) = &window.schedule {
        runinator_scheduling::validate(schedule)
            .map_err(|error| crate::errors::FREEZE_WINDOW_INVALID.error(error))?;
    }
    Ok(())
}

/// the range a backfill is allowed to replay: bounded, ordered, and in the past. a forward-dated
/// backfill would create runs for slots the trigger loop is about to fire on its own.
pub fn validate_backfill_request(request: &BackfillRequest) -> Result<(), SendableError> {
    if request.to <= request.from {
        return Err(crate::errors::BACKFILL_INVALID_RANGE.error("to must be after from"));
    }
    if request.from > Utc::now() {
        return Err(crate::errors::BACKFILL_INVALID_RANGE.error("from must be in the past"));
    }
    Ok(())
}

pub async fn delete_workflow_trigger<T: ScheduleStore>(
    db: &T,
    trigger_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow_trigger(trigger_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow trigger deleted".into(),
    })
}

pub async fn create_workflow_run_for_trigger<T: ScheduleStore + RuntimeStore + WorkflowVmStore>(
    db: &T,
    trigger_id: Uuid,
    parameters: Value,
    debug: bool,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    let Some(trigger) = db.fetch_workflow_trigger(trigger_id).await? else {
        return Err(runinator_runtime::errors::WORKFLOW_TRIGGER_NOT_FOUND.error(trigger_id));
    };
    let workflow_snapshot = support::fetch_workflow_snapshot(db, trigger.workflow_id).await?;
    let mut state = trigger_state(&trigger);
    if debug {
        let debug_state = runinator_models::json!({
            "enabled": true,
            "paused": false,
            "step_requested": false
        });
        if let Some(object) = state.as_object_mut() {
            object.insert("debug".into(), debug_state);
        }
    }
    super::runs::create_workflow_vm_run(
        db,
        super::runs::WorkflowVmRunRequest {
            workflow_id: trigger.workflow_id,
            workflow_snapshot,
            parameters,
            state,
            name: None,
            provenance: runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Manual),
                actor_type: Some(runinator_models::replicas::TriggerActorType::User),
                actor_replica_id,
                actor_display_name,
                request_host: None,
                request_ip: None,
                metadata: trigger.metadata.clone(),
            },
            pipeline_run_id: None,
            start_node_id: None,
        },
    )
    .await
}

fn trigger_state(trigger: &WorkflowTrigger) -> Value {
    runinator_models::json!({
        "control": { "pause_requested": false },
        "trigger": {
            "id": trigger.id,
            "kind": trigger.kind,
            "metadata": trigger.metadata
        }
    })
}
