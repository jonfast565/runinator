use super::support;
use super::*;

pub async fn create_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
    parameters: Value,
    debug: bool,
    name: Option<String>,
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
        .create_workflow_run(workflow_id, workflow_snapshot, parameters, state, trimmed)
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
    ready_node_id: i64,
    scheduler_id: String,
    next_ready: Option<(i64, String, chrono::DateTime<Utc>)>,
) -> Result<TaskResponse, SendableError> {
    let Some(ready_node) = db.fetch_ready_node(ready_node_id).await? else {
        return Err(crate::errors::READY_NODE_NOT_FOUND.error(ready_node_id));
    };
    if ready_node.claimed_by.as_deref() != Some(scheduler_id.as_str()) {
        return Err(crate::errors::READY_NODE_NOT_CLAIMED.error(ready_node_id));
    }
    let disposition = crate::orchestration::process_ready_node(db, &ready_node).await?;
    if disposition == crate::orchestration::ReadyNodeDisposition::KeepClaim {
        return Ok(TaskResponse {
            success: true,
            message: "Ready node remains claimed until it is due".into(),
        });
    }
    if !db.complete_ready_node(ready_node_id, scheduler_id).await? {
        return Err(crate::errors::READY_NODE_NOT_CLAIMED.error(ready_node_id));
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
    ready_node_id: i64,
    driver_id: String,
) -> Result<Option<i64>, SendableError> {
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
    err: &(dyn std::error::Error + Send + Sync),
) -> Result<(), SendableError> {
    log::error!(
        "Reducer failed for ready node {} (workflow run {}, node {}): {}",
        ready_node.id,
        ready_node.workflow_run_id,
        ready_node.node_id,
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

/// publish a wake for each ready node still pending drive. doubles as the durable backstop: the
/// broker dedupes wakes already in flight, so re-announcing an undriven node is harmless.
pub async fn publish_pending_wakes<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    limit: i64,
) -> Result<(), SendableError> {
    let now = Utc::now();
    let pending = db.fetch_pending_ready_nodes(now, limit).await?;
    for node in pending {
        let command = runinator_comm::WakeCommand::new(
            node.id,
            node.workflow_run_id,
            node.node_id,
            node.ready_at,
            node.source_event_id,
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
    workflow_run_id: i64,
    scheduler_id: String,
    lease_until: chrono::DateTime<Utc>,
) -> Result<bool, SendableError> {
    db.renew_workflow_run_claim(workflow_run_id, scheduler_id, lease_until)
        .await
}

pub async fn release_workflow_run_claim<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    scheduler_id: String,
) -> Result<(), SendableError> {
    db.release_workflow_run_claim(workflow_run_id, scheduler_id)
        .await
}

pub async fn fetch_recent_workflow_runs<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_recent_workflow_runs().await
}

pub async fn fetch_workflow_runs_for_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
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
    workflow_run_id: i64,
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

pub async fn set_workflow_run_name<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    name: Option<String>,
) -> Result<TaskResponse, SendableError> {
    let trimmed = support::normalized_run_name(name);
    db.set_workflow_run_name(workflow_run_id, trimmed).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run renamed".into(),
    })
}
