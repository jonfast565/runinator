use super::support;
use super::*;
use uuid::Uuid;

pub async fn upsert_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger: &WorkflowTrigger,
) -> Result<WorkflowTrigger, SendableError> {
    db.upsert_workflow_trigger(trigger).await
}

pub async fn fetch_workflow_triggers<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_triggers(workflow_id).await
}

pub async fn fetch_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<Option<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_trigger(trigger_id).await
}

pub async fn fetch_due_workflow_triggers<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_due_workflow_triggers(Utc::now()).await
}

pub async fn claim_due_workflow_trigger_firings<T: DatabaseImpl>(
    db: &T,
    scheduler_id: String,
    limit: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    let runs = db
        .claim_due_workflow_trigger_firings(scheduler_id, Utc::now(), limit)
        .await?;
    for run in &runs {
        support::enqueue_start_ready_node(db, run).await?;
    }
    Ok(runs)
}

pub async fn delete_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow_trigger(trigger_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow trigger deleted".into(),
    })
}

pub async fn create_workflow_run_for_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: Uuid,
    parameters: Value,
    debug: bool,
    actor_replica_id: Option<Uuid>,
    actor_display_name: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    let Some(trigger) = db.fetch_workflow_trigger(trigger_id).await? else {
        return Err(crate::errors::WORKFLOW_TRIGGER_NOT_FOUND.error(trigger_id));
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
    let run = db
        .create_workflow_run(
            trigger.workflow_id,
            workflow_snapshot,
            parameters,
            state,
            None,
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Manual),
                actor_type: Some(runinator_models::replicas::TriggerActorType::User),
                actor_replica_id,
                actor_display_name,
                request_host: None,
                request_ip: None,
                metadata: trigger.metadata.clone(),
            },
        )
        .await?;
    support::enqueue_start_ready_node(db, &run).await?;
    Ok(run)
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
