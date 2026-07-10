use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

const RECORD_TYPE: &str = "workflow_mutex";
const DEFAULT_POLL_INTERVAL: i64 = 5;

pub(super) struct MutexParams {
    pub(super) name: String,
    pub(super) poll_interval: i64,
    // true when this node releases a held lock (an end-of-section release node) instead of acquiring.
    pub(super) release: bool,
    // optional hold lease lifetime; when set, an acquired lock auto-expires this long after
    // acquisition regardless of the holder run's state. decoupled from the node timeout, which bounds
    // only the wait-to-acquire.
    pub(super) hold_timeout: Option<i64>,
}

pub(super) fn parse_mutex_params(node: &WorkflowNode) -> MutexParams {
    let params: Value = node.parameters.clone().into();
    MutexParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        poll_interval: params
            .get("poll_interval_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_POLL_INTERVAL),
        release: params
            .get("release")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        hold_timeout: params.get("hold_timeout_seconds").and_then(Value::as_i64),
    }
}

/// the run currently holding a mutex record, if the record names one.
pub(super) fn holder_run_id(record: &Value) -> Option<Uuid> {
    record
        .get("held_by_run_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok())
}

/// true when a mutex automation record is currently held by a run other than `skip_run_id`.
/// a record is considered released when it carries a `released_at` field.
pub(super) fn record_is_held_by_other(record: &Value, skip_run_id: Uuid) -> bool {
    if record.get("released_at").is_some() {
        return false;
    }
    holder_run_id(record).is_some_and(|id| id != skip_run_id)
}

/// true when a lease carries an explicit `hold` deadline that has passed, making it reclaimable
/// regardless of the holder run's status. only a node that declares an explicit `hold` timeout
/// stamps a `lease_deadline`; a lock held with no `hold` never expires this way and is instead
/// released when its holder run reaches a terminal state (see `holder_run_is_active`) or when an
/// end-of-section release node runs. this lets a legitimate critical section run to completion
/// however long it takes, while a bounded `hold` still self-heals a wedged holder.
pub(super) fn lease_is_expired(record: &Value) -> bool {
    let Some(deadline) = record.get("lease_deadline").and_then(Value::as_i64) else {
        return false;
    };
    Utc::now().timestamp() > deadline
}

/// true when the run holding a lease is still active. a lease whose holder run has reached a
/// terminal state (or no longer exists) is stale: the run ended without releasing — e.g. via
/// cancellation, a crash, or a code path that bypassed the terminal-release hook — so its lock is
/// reclaimable. this keeps a named mutex from deadlocking waiters behind a finished run.
async fn holder_run_is_active<T: DatabaseImpl>(
    db: &T,
    record: &Value,
) -> Result<bool, SendableError> {
    let Some(run_id) = holder_run_id(record) else {
        return Ok(false);
    };
    match db.fetch_workflow_run(run_id).await? {
        Some(run) => Ok(!run.status.is_terminal()),
        None => Ok(false),
    }
}

async fn mutex_is_locked<T: DatabaseImpl>(
    db: &T,
    name: &str,
    skip_run_id: Uuid,
) -> Result<bool, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    for record in &records {
        if record.get("name").and_then(Value::as_str) != Some(name) {
            continue;
        }
        if !record_is_held_by_other(record, skip_run_id) {
            continue;
        }
        // an expired lease is reclaimable even if the holder run is still non-terminal: this is what
        // keeps a wedged holder from deadlocking the lock and timing out every waiter behind it.
        if lease_is_expired(record) {
            continue;
        }
        if !holder_run_is_active(db, record).await? {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

/// release every mutex lease held by `run_id` by stamping `released_at`. called when a run reaches a
/// terminal state so a shared named lock passes to the next waiter. idempotent: records already
/// released or held by a different run are left untouched.
pub(super) async fn release_run_mutexes<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
) -> Result<(), SendableError> {
    release_run_leases(db, run_id, None).await
}

/// release only the lease(s) named `name` held by `run_id`. drives an end-of-section release node so
/// the critical section ends before the run terminates. idempotent and a no-op when the run holds no
/// such lock.
pub(super) async fn release_run_mutex_named<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    name: &str,
) -> Result<(), SendableError> {
    release_run_leases(db, run_id, Some(name)).await
}

/// stamp `released_at` on every unreleased lease held by `run_id`, optionally restricted to a single
/// `name`. shared by the terminal-release hook and the end-of-section release node.
async fn release_run_leases<T: DatabaseImpl>(
    db: &T,
    run_id: Uuid,
    name: Option<&str>,
) -> Result<(), SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    for record in records {
        if holder_run_id(&record) != Some(run_id) || record.get("released_at").is_some() {
            continue;
        }
        if let Some(name) = name {
            if record.get("name").and_then(Value::as_str) != Some(name) {
                continue;
            }
        }
        let Some(id) = record
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Uuid>().ok())
        else {
            continue;
        };
        let mut released = record.clone();
        if let Some(object) = released.as_object_mut() {
            object.insert(
                "released_at".into(),
                runinator_models::json!(Utc::now().timestamp()),
            );
        }
        db.update_automation_record(RECORD_TYPE.into(), id, released)
            .await?;
    }
    Ok(())
}

/// true when `run_id` already holds a live, unexpired lease for `name`. re-reaching an acquire node in
/// a loop reinforces this lock rather than recording a second lease. an expired hold does not count,
/// so a run whose bounded hold lapsed re-contends normally instead of assuming it still holds.
async fn run_holds_mutex<T: DatabaseImpl>(
    db: &T,
    name: &str,
    run_id: Uuid,
) -> Result<bool, SendableError> {
    let records = db
        .fetch_automation_records(RECORD_TYPE.into(), None, None)
        .await?;
    Ok(records.iter().any(|record| {
        record.get("name").and_then(Value::as_str) == Some(name)
            && record.get("released_at").is_none()
            && holder_run_id(record) == Some(run_id)
            && !lease_is_expired(record)
    }))
}

// record a lease for `name` held by `run_id`. mutual exclusion relies on the ws ingress consumer
// driving the reducer one node at a time per replica, so the check-then-acquire above is atomic
// within a replica; a multi-replica ws deployment would need a db-level compare-and-swap to make it
// airtight across replicas.
async fn acquire_mutex<T: DatabaseImpl>(
    db: &T,
    name: &str,
    run_id: Uuid,
    ttl_seconds: Option<i64>,
) -> Result<Option<Uuid>, SendableError> {
    let acquired_at = Utc::now().timestamp();
    let mut record = runinator_models::json!({
        "name": name,
        "held_by_run_id": run_id,
        "acquired_at": acquired_at,
    });
    // stamp an absolute expiry so a holder that later wedges in a non-terminal state cannot hold the
    // lock past this deadline; waiters reclaim it once it lapses.
    if let (Some(ttl), Some(object)) = (ttl_seconds, record.as_object_mut()) {
        object.insert(
            "lease_deadline".into(),
            runinator_models::json!(acquired_at + ttl),
        );
    }
    let inserted = db
        .create_automation_record(RECORD_TYPE.into(), record)
        .await?;
    Ok(inserted
        .get("id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok()))
}

/// acquire `name` for `run_id`, or reinforce an existing hold. re-reaching an acquire node in a loop
/// must not record a second lease for a lock the run already holds; it simply keeps the current one.
async fn acquire_or_reinforce<T: DatabaseImpl>(
    db: &T,
    name: &str,
    run_id: Uuid,
    hold_timeout: Option<i64>,
) -> Result<(), SendableError> {
    if run_holds_mutex(db, name, run_id).await? {
        return Ok(());
    }
    acquire_mutex(db, name, run_id, hold_timeout).await?;
    Ok(())
}

async fn enqueue_mutex_poll<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
    interval: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(interval);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "mutex_poll",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), poll_at)
        .await?;
    Ok(())
}

/// process a mutex node. an acquire node tries to take a named distributed lease, parking and polling
/// until it is free or the wait timeout elapses; a release node (`release: true`) ends the section by
/// releasing the run's hold on the named lease and completing inline.
pub(super) async fn process_mutex_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = parse_mutex_params(node);

    // an end-of-section release node: drop this run's hold on the named lock and complete. no acquire,
    // no park. a no-op when the run holds no such lock (idempotent when re-reached in a loop).
    if params.release {
        let node_run = db
            .create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?;
        release_run_mutex_named(db, workflow_run.id, &params.name).await?;
        let output = MutexOutput {
            name: params.name,
            acquired: false,
            released: true,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("mutex_released".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));

    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        if timed_out_since_created(node, node_run) {
            time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Mutex timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        if mutex_is_locked(db, &params.name, workflow_run.id).await? {
            enqueue_mutex_poll(db, workflow_run.id, node, params.poll_interval).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }
        // lock is free; record the acquisition and succeed.
        acquire_or_reinforce(db, &params.name, workflow_run.id, params.hold_timeout).await?;
        let output = MutexOutput {
            name: params.name,
            acquired: true,
            released: false,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("mutex_acquired".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // first visit.
    if !mutex_is_locked(db, &params.name, workflow_run.id).await? {
        let node_run = db
            .create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?;
        acquire_or_reinforce(db, &params.name, workflow_run.id, params.hold_timeout).await?;
        let output = MutexOutput {
            name: params.name,
            acquired: true,
            released: false,
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("mutex_acquired".into()),
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // park and poll.
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let state = MutexState {
        name: params.name.clone(),
        poll_interval: params.poll_interval,
        deadline_unix: node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some("mutex_waiting".into()),
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
    enqueue_mutex_poll(db, workflow_run.id, node, params.poll_interval).await?;
    arm_node_timeout(db, workflow_run.id, node).await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct MutexHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for MutexHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_mutex_node(
                ctx.db,
                ctx.workflow_run,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await
        }
    }
}
