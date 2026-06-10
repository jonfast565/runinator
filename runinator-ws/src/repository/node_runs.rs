use super::support;
use super::*;
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
) -> Result<TaskResponse, SendableError> {
    db.claim_workflow_node_run_executor(workflow_node_run_id, replica_id, claimed_at)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow node run executor claimed".into(),
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

pub async fn apply_workflow_result_event<T: DatabaseImpl>(
    db: &T,
    event: &WorkflowResultEvent,
) -> Result<bool, SendableError> {
    let applied = db.apply_workflow_result_event(event).await?;
    if !applied {
        return Ok(false);
    }
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
    Ok(true)
}

pub async fn create_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    parameters: Value,
) -> Result<WorkflowNodeRun, SendableError> {
    db.create_workflow_node_run(workflow_run_id, node_id, parameters)
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
