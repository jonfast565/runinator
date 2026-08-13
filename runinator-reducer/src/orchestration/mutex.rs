use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;
use runinator_store::workflow_mutex::{WorkflowMutexClaim, WorkflowMutexWake};

const DEFAULT_POLL_INTERVAL: i64 = 5;

pub(super) struct MutexParams {
    pub(super) name: String,
    pub(super) poll_interval: i64,
    // true when this node releases a held lock instead of acquiring one.
    pub(super) release: bool,
    // maximum expected section duration. expiry is diagnostic while the holder remains active.
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

pub(super) struct MutexOps<'a, T: ReducerStore> {
    db: &'a T,
}

impl<'a, T: ReducerStore> MutexOps<'a, T> {
    pub(super) fn new(db: &'a T) -> Self {
        Self { db }
    }

    /// release every cursor-owned mutex for a terminal run and durably wake each fifo successor.
    pub(super) async fn release_run_mutexes(&self, run_id: Uuid) -> Result<(), SendableError> {
        let wakes = self
            .db
            .release_workflow_mutexes(run_id, Utc::now().timestamp())
            .await?;
        for wake in wakes {
            enqueue_mutex_wake(self.db, wake).await?;
        }
        Ok(())
    }

    async fn release_named(
        &self,
        run_id: Uuid,
        cursor_id: Uuid,
        name: &str,
    ) -> Result<(), SendableError> {
        if let Some(wake) = self
            .db
            .release_workflow_mutex(name.to_string(), run_id, cursor_id, Utc::now().timestamp())
            .await?
        {
            enqueue_mutex_wake(self.db, wake).await?;
        }
        Ok(())
    }

    async fn enqueue_mutex_poll(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
        interval: i64,
    ) -> Result<(), SendableError> {
        let poll_at = Utc::now() + chrono::Duration::seconds(interval.max(1));
        let event = NewOrchestrationEvent::new(
            ctx.workflow_run.id,
            Some(ctx.node.id.clone()),
            "mutex_poll",
            runinator_models::json!({ "node_id": ctx.node.id }),
        )
        .for_cursor(ctx.cursor.id);
        self.db
            .enqueue_ready_node(event, ctx.node.id.clone(), poll_at)
            .await?;
        Ok(())
    }

    async fn claim(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
        node_run: &WorkflowNodeRun,
        params: &MutexParams,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = self
            .db
            .claim_workflow_mutex(
                WorkflowMutexClaim {
                    name: params.name.clone(),
                    workflow_run_id: ctx.workflow_run.id,
                    workflow_node_run_id: node_run.id,
                    cursor_id: ctx.cursor.id,
                    node_id: ctx.node.id.clone(),
                    hold_deadline_unix: params
                        .hold_timeout
                        .map(|timeout| now.saturating_add(timeout.max(0))),
                    enqueued_at_unix: node_run.created_at.timestamp(),
                },
                now,
            )
            .await?;
        if let Some(wake) = result.wake {
            enqueue_mutex_wake(self.db, wake).await?;
        }
        if result.holder_overdue && !result.acquired {
            tracing::warn!(
                mutex = %params.name,
                waiting_run_id = %ctx.workflow_run.id,
                "workflow mutex holder exceeded hold timeout but remains active"
            );
        }
        Ok(result.acquired)
    }

    pub(super) async fn reduce_node(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let params = parse_mutex_params(ctx.node);

        if params.release {
            let node_run = self
                .db
                .create_workflow_node_run(
                    ctx.workflow_run.id,
                    ctx.node.id.clone(),
                    ctx.node.parameters.clone().into(),
                    super::context::most_recently_finished_node_run(ctx.node_runs),
                    Some(ctx.cursor),
                )
                .await?;
            self.release_named(ctx.workflow_run.id, ctx.cursor.id, &params.name)
                .await?;
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(
                    MutexOutput {
                        name: params.name,
                        acquired: false,
                        released: true,
                    }
                    .to_wire_value()?,
                ),
                Some("mutex_released".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));
        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting)
            && timed_out_since_created(ctx.timing(), node_run)
        {
            self.db.remove_workflow_mutex_waiter(node_run.id).await?;
            time_out(ctx, node_run, "Mutex timed out").await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        let node_run = match latest {
            Some(node_run) => node_run.clone(),
            None => {
                self.db
                    .create_workflow_node_run(
                        ctx.workflow_run.id,
                        ctx.node.id.clone(),
                        ctx.node.parameters.clone().into(),
                        super::context::most_recently_finished_node_run(ctx.node_runs),
                        Some(ctx.cursor),
                    )
                    .await?
            }
        };

        if self.claim(ctx, &node_run, &params).await? {
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(
                    MutexOutput {
                        name: params.name,
                        acquired: true,
                        released: false,
                    }
                    .to_wire_value()?,
                ),
                Some("mutex_acquired".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        if node_run.status != WorkflowStatus::Waiting {
            self.db
                .update_workflow_node_run(
                    node_run.id,
                    WorkflowStatus::Waiting,
                    Some(node_run.attempt + 1),
                    None,
                    None,
                    Some(
                        MutexState {
                            name: params.name,
                            poll_interval: params.poll_interval,
                            deadline_unix: ctx
                                .node
                                .timeout_seconds
                                .map(|timeout| Utc::now().timestamp() + timeout),
                        }
                        .to_wire_value()?,
                    ),
                    Some("mutex_waiting".into()),
                    None,
                )
                .await?;
            self.db
                .update_workflow_run_status(
                    ctx.workflow_run.id,
                    WorkflowStatus::Waiting,
                    Some(ctx.node.id.clone()),
                    None,
                    None,
                )
                .await?;
            arm_node_timeout(ctx).await?;
        }
        self.enqueue_mutex_poll(ctx, params.poll_interval).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) async fn enqueue_mutex_wake<T: ReducerStore>(
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

pub(super) struct MutexHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for MutexHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        MutexOps::new(ctx.db).reduce_node(ctx).await
    }
}
