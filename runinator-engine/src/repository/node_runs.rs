use super::support;
use super::*;
use runinator_models::notifications::NotificationDeliveryStatus;
use uuid::Uuid;

pub async fn fetch_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Option<(WorkflowRun, Vec<WorkflowNodeRun>)>, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(None);
    };
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    Ok(Some((run, nodes)))
}

pub async fn fetch_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
) -> Result<Option<WorkflowNodeRun>, SendableError> {
    db.fetch_workflow_node_run(workflow_node_run_id).await
}

pub async fn claim_workflow_node_run_executor<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
    replica_id: Uuid,
    claimed_at: DateTime<Utc>,
    stale_before: DateTime<Utc>,
) -> Result<TaskResponse, SendableError> {
    // liveness is a platform policy, not the caller's: a worker knows its own action deadline but not
    // how long another replica may go quiet before it counts as dead. deriving the heartbeat cutoff
    // here keeps that one definition (`REPLICA_STALE_SECONDS`) shared with replica listing and action
    // routing, and keeps it off the claim's wire payload.
    let heartbeat_stale_before = claimed_at - Duration::seconds(REPLICA_STALE_SECONDS);
    let acquired = db
        .claim_workflow_node_run_executor(
            workflow_node_run_id,
            replica_id,
            claimed_at,
            stale_before,
            heartbeat_stale_before,
        )
        .await?;
    Ok(TaskResponse {
        success: acquired,
        message: if acquired {
            "Workflow node run executor claimed".into()
        } else {
            "Workflow node run executor already held by a live executor".into()
        },
    })
}

pub async fn release_workflow_node_run_executor<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
    replica_id: Uuid,
    released_at: DateTime<Utc>,
) -> Result<TaskResponse, SendableError> {
    db.release_workflow_node_run_executor(workflow_node_run_id, replica_id, released_at)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow node run executor released".into(),
    })
}

pub async fn append_workflow_node_run_chunk<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
    chunk: &NewRunChunk,
) -> Result<WorkflowNodeRunChunk, SendableError> {
    db.append_workflow_node_run_chunk(workflow_node_run_id, chunk)
        .await
}

pub async fn fetch_workflow_node_run_chunks<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
    db.fetch_workflow_node_run_chunks(workflow_node_run_id, cursor, limit)
        .await
}

pub async fn add_workflow_node_run_artifact<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
    artifact: &NewRunArtifact,
) -> Result<WorkflowNodeRunArtifact, SendableError> {
    db.add_workflow_node_run_artifact(workflow_node_run_id, artifact)
        .await
}

pub async fn fetch_workflow_node_run_artifacts<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: Uuid,
) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
    db.fetch_workflow_node_run_artifacts(workflow_node_run_id)
        .await
}

pub async fn fetch_workflow_run_artifacts<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Vec<runinator_models::workflows::WorkflowRunArtifact>, SendableError> {
    db.fetch_workflow_run_artifacts(workflow_run_id).await
}

pub async fn fetch_run_transitions<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<Vec<runinator_models::orchestration::NodeTransition>, SendableError> {
    db.fetch_run_transitions(workflow_run_id).await
}

pub async fn fetch_node_transition_stats<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
    node_id: Option<String>,
) -> Result<Vec<runinator_models::orchestration::NodeTransitionStat>, SendableError> {
    db.fetch_node_transition_stats(workflow_id, node_id).await
}

pub async fn apply_workflow_result_event<T: DatabaseImpl>(
    db: &T,
    event: &WorkflowResultEvent,
) -> Result<bool, SendableError> {
    // A RexRap provider task is launched from a node but deliberately outlives that node's cursor.
    // Its terminal result must therefore never use the generic node-result path: that would replay
    // the launch node's transition after the workflow has already moved on.
    if let Some(task_run_id) = event.task_run_id {
        return apply_workflow_task_result(db, event, task_run_id).await;
    }
    // a notification delivery reuses the action path but owns no node run; settle its delivery row
    // and stop, so the generic node-run apply never sees a synthetic node run id.
    if let Some(delivery_id) = event.notification_delivery_id {
        return apply_notification_delivery_result(db, event, delivery_id).await;
    }
    // one durable call of a resumable invocation. unlike a notification delivery this *does* own a
    // node run — the invocation's — so the split is only about the terminal status: it settles the
    // call rather than the node run, because the node stays `Running` across every call its program
    // makes. chunks and artifacts still flow to the node run on the normal path.
    if let Some(call_id) = event.invocation_call_id {
        return apply_invocation_call_result(db, event, call_id).await;
    }
    let applied = db.apply_workflow_result_event(event).await?;
    // enqueue the drive even when the event is a duplicate: a redelivery usually means a prior
    // attempt failed between persisting the event and enqueueing this ready node, and skipping it
    // here would strand the run in `running` with no backstop. a spurious drive is harmless.
    if let WorkflowResultEventKind::Status { status, .. } = &event.kind
        && status.is_terminal()
    {
        support::enqueue_node_ready(
            db,
            event.workflow_run_id,
            event.node_id.clone(),
            "workflow_result_status",
            Utc::now(),
            runinator_models::json!({
                "workflow_node_run_id": event.workflow_node_run_id,
                "status": status,
            }),
        )
        .await?;
    }
    Ok(applied)
}

async fn apply_workflow_task_result<T: DatabaseImpl>(
    db: &T,
    event: &WorkflowResultEvent,
    task_run_id: Uuid,
) -> Result<bool, SendableError> {
    let WorkflowResultEventKind::Status {
        status,
        output_json,
        message,
    } = &event.kind
    else {
        // Logs and artifacts still belong to the launch node's familiar transport surface. They
        // carry no terminal transition, so the generic path is safe for them.
        return db.apply_workflow_result_event(event).await;
    };
    if !status.is_terminal() {
        return Ok(false);
    }
    let Some(task) = db.fetch_workflow_task_run(task_run_id).await? else {
        return Ok(false);
    };
    // A redelivered or older-attempt result may never regress a task that already settled.
    if task.status.is_terminal() || (event.attempt > 0 && event.attempt < task.attempt) {
        return Ok(false);
    }
    db.update_workflow_task_run(
        task_run_id,
        *status,
        Some(event.attempt),
        output_json.clone(),
        message.clone(),
    )
    .await?;
    Ok(true)
}

/// settle the call an invocation is parked on, then drive the node so its program resumes.
///
/// the drive is enqueued even when the settle was a no-op. a redelivery usually means a prior
/// attempt failed between recording the result and enqueueing the ready node, and skipping it would
/// strand the invocation parked on a call that already finished; a spurious drive re-reads the same
/// settled call and is harmless.
async fn apply_invocation_call_result<T: DatabaseImpl>(
    db: &T,
    event: &WorkflowResultEvent,
    call_id: Uuid,
) -> Result<bool, SendableError> {
    let WorkflowResultEventKind::Status {
        status,
        output_json,
        message,
    } = &event.kind
    else {
        // a chunk or artifact from a call belongs to the invocation's node run, which is where the
        // generic path already writes it.
        return db.apply_workflow_result_event(event).await;
    };
    if !status.is_terminal() {
        return Ok(false);
    }
    let applied = db
        .settle_invocation_call(
            call_id,
            event.attempt,
            *status,
            output_json.clone(),
            message.clone(),
        )
        .await?;
    support::enqueue_node_ready(
        db,
        event.workflow_run_id,
        event.node_id.clone(),
        "invocation_call_settled",
        Utc::now(),
        runinator_models::json!({
            "workflow_node_run_id": event.workflow_node_run_id,
            "invocation_call_id": call_id,
            "status": status,
        }),
    )
    .await?;
    Ok(applied)
}

/// settle the durable delivery row for a notification the worker just attempted. only the terminal
/// status matters; chunk/artifact events from a delivery action carry no state worth keeping.
async fn apply_notification_delivery_result<T: DatabaseImpl>(
    db: &T,
    event: &WorkflowResultEvent,
    delivery_id: Uuid,
) -> Result<bool, SendableError> {
    let WorkflowResultEventKind::Status {
        status, message, ..
    } = &event.kind
    else {
        return Ok(false);
    };
    if !status.is_terminal() {
        return Ok(false);
    }
    let delivery_status = if *status == WorkflowStatus::Succeeded {
        NotificationDeliveryStatus::Delivered
    } else {
        NotificationDeliveryStatus::Failed
    };
    db.mark_notification_delivery(delivery_id, delivery_status, message.clone())
        .await?;
    Ok(true)
}

pub async fn create_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    parameters: Value,
    prev_node_run_id: Option<Uuid>,
) -> Result<WorkflowNodeRun, SendableError> {
    db.create_workflow_node_run(workflow_run_id, node_id, parameters, prev_node_run_id, None)
        .await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    node_run_id: Uuid,
    status: WorkflowStatus,
    attempt: Option<i64>,
    parameters: Option<Value>,
    output_json: Option<Value>,
    state: Option<Value>,
    transition_reason: Option<String>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_node_run(
        node_run_id,
        status,
        attempt,
        parameters,
        output_json,
        state,
        transition_reason,
        message,
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow node run updated".into(),
    })
}

pub async fn resolve_workflow_input<T: DatabaseImpl>(
    db: &T,
    node_run_id: Uuid,
    output_json: Value,
    resolved_by: Option<String>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    let Some(node_run) = db.fetch_workflow_node_run(node_run_id).await? else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workflow node run {node_run_id} not found"),
        )));
    };
    let Some(workflow_run) = db.fetch_workflow_run(node_run.workflow_run_id).await? else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workflow run {} not found", node_run.workflow_run_id),
        )));
    };
    let Some(workflow) = workflow_run.workflow_snapshot.as_ref() else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workflow snapshot for run {} not found", workflow_run.id),
        )));
    };
    let Some(node) = workflow
        .definition
        .nodes
        .iter()
        .find(|candidate| candidate.id == node_run.node_id)
    else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workflow node {} not found", node_run.node_id),
        )));
    };
    if node.kind != WorkflowNodeKind::Input {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Workflow node {} is not an input node", node_run.node_id),
        )));
    }

    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        Some(output_json.clone()),
        Some(node_run.state.clone()),
        Some("input_resolved".into()),
        message.clone(),
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(node.id.clone()),
        None,
        message.clone(),
    )
    .await?;
    support::enqueue_node_ready(
        db,
        workflow_run.id,
        node.id.clone(),
        "input_resolved",
        Utc::now(),
        runinator_models::json!({
            "workflow_node_run_id": node_run.id,
            "resolved_by": resolved_by,
            "value": output_json,
        }),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow input resolved".into(),
    })
}
