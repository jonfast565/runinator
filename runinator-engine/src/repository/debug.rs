use super::support;
use super::*;
use uuid::Uuid;

/// how many speculative branches one run may carry at once. they live in the run's state blob, so an
/// unbounded fork button would grow a single row without limit.
const MAX_SPECULATIVE_CURSORS: usize = 8;

/// a debug-enabled, non-terminal run loaded once and shared by every verb handler below. `db` and
/// `run` are genuinely invariant per session — every method here reads or rewrites the same run —
/// which is what replaced the `require_debug_run` call each of these used to repeat individually.
struct DebugSession<'a, T: DatabaseImpl> {
    db: &'a T,
    run: WorkflowRun,
}

/// unpark every thread of control, and the run-scoped mirror with them.
///
/// clearing only the flat frame leaves a fan-out's branches parked forever: each carries its own
/// runtime, and the reducer reads *those*. resume and cancel must reach all of them.
fn release_every_cursor(run_state: &mut WorkflowRunState) {
    for cursor in &mut run_state.cursors {
        if let Some(runtime) = cursor.debug.as_mut() {
            runtime.paused = false;
            runtime.step_requested = false;
        }
    }
    if let Some(debug) = run_state.debug.as_mut() {
        debug.runtime.paused = false;
        debug.runtime.step_requested = false;
    }
}

/// the cursor a debug verb addresses: the caller's choice, else the one the operator is looking at
/// — the first parked branch, else the primary.
///
/// resolving here rather than in each verb is what lets every existing single-cursor client keep
/// sending the same payloads against a run that has since fanned out.
fn resolve_target_cursor(
    run_state: &WorkflowRunState,
    requested: Option<Uuid>,
) -> Result<Uuid, SendableError> {
    if let Some(id) = requested {
        return run_state
            .cursor(id)
            .map(|cursor| cursor.id)
            .ok_or_else(|| crate::errors::DEBUG_CURSOR_NOT_FOUND.error(id));
    }
    run_state
        .cursors
        .iter()
        .find(|cursor| run_state.cursor_debug(cursor.id).paused)
        .or_else(|| run_state.primary_cursor())
        .map(|cursor| cursor.id)
        .ok_or_else(|| crate::errors::DEBUG_NO_ACTIVE_NODE.bare())
}

impl<'a, T: DatabaseImpl> DebugSession<'a, T> {
    async fn load(db: &'a T, workflow_run_id: Uuid) -> Result<Self, SendableError> {
        let run = require_debug_run(db, workflow_run_id).await?;
        Ok(Self { db, run })
    }

    /// dispatch a single canonical [`DebugVerb`] against this session's run. every debug operation
    /// funnels through here so the per-verb behavior lives in exactly one place.
    async fn apply(&self, verb: DebugVerb) -> Result<TaskResponse, SendableError> {
        match verb {
            DebugVerb::Step { cursor } => self.step_cursor(cursor).await,
            DebugVerb::Continue { cursor } => self.continue_cursor(cursor).await,
            DebugVerb::RunToCursor { node_id, cursor } => self.run_to_cursor(node_id, cursor).await,
            DebugVerb::Skip {
                output,
                message,
                cursor,
            } => self.skip_node(output, message, cursor).await,
            DebugVerb::Rerun { parameters, cursor } => self.rerun_node(parameters, cursor).await,
            DebugVerb::SetBreakpoints { breakpoints } => self.set_breakpoints(breakpoints).await,
            DebugVerb::SetMode { mode } => self.set_mode(mode).await,
            DebugVerb::Fork {
                from_cursor,
                at_node,
                label,
                context_patch,
            } => {
                self.fork_cursor(from_cursor, at_node, label, context_patch)
                    .await
            }
            DebugVerb::RetireCursor { cursor } => self.retire_cursor(cursor).await,
            DebugVerb::ArmForReal {
                cursor,
                node_id,
                armed,
            } => self.arm_node(cursor, node_id, armed).await,
        }
    }

    /// fork a speculative "what if" branch beside the real ones.
    async fn fork_cursor(
        &self,
        from_cursor: Option<Uuid>,
        at_node: Option<String>,
        label: Option<String>,
        context_patch: Value,
    ) -> Result<TaskResponse, SendableError> {
        let run = &self.run;
        let mut run_state = run.execution_state.clone();
        run_state
            .debug
            .get_or_insert_with(DebugFrame::default)
            .config
            .enabled = true;
        if run_state
            .cursors
            .iter()
            .filter(|c| c.is_speculative())
            .count()
            >= MAX_SPECULATIVE_CURSORS
        {
            return Err(crate::errors::DEBUG_FORK_INVALID.error(format!(
                "at most {MAX_SPECULATIVE_CURSORS} speculative branches per run"
            )));
        }
        let parent = resolve_target_cursor(&run_state, from_cursor)?;
        let entry = match at_node {
            Some(node_id) => node_id,
            None => run_state
                .cursor(parent)
                .map(|cursor| cursor.node_id().to_string())
                .ok_or_else(|| crate::errors::DEBUG_FORK_INVALID.error(parent))?,
        };
        // the fork must exist in the graph, or it would park on a node no drive can resolve.
        if let Some(snapshot) = run.workflow_snapshot.as_ref()
            && !snapshot
                .definition
                .nodes
                .iter()
                .any(|node| node.id == entry)
        {
            return Err(
                crate::errors::DEBUG_FORK_INVALID.error(format!("no node {entry} in this run"))
            );
        }
        let forked = run_state
            .fork_speculative(parent, &entry, label, context_patch)
            .ok_or_else(|| crate::errors::DEBUG_FORK_INVALID.error(parent))?;

        self.db
            .update_workflow_run_status(
                run.id,
                run.status,
                run.active_node_id.clone(),
                Some(run_state),
                Some(format!("Forked speculative branch at {entry}")),
            )
            .await?;
        support::enqueue_node_ready_for_cursor(
            self.db,
            run.id,
            forked,
            entry,
            "debug_fork",
            Utc::now(),
        )
        .await?;
        Ok(TaskResponse {
            success: true,
            message: forked.to_string(),
        })
    }

    /// abandon a speculative branch and everything forked from it.
    async fn retire_cursor(&self, cursor: Uuid) -> Result<TaskResponse, SendableError> {
        let run = &self.run;
        let mut run_state = run.execution_state.clone();
        if !run_state.is_speculative(cursor) {
            return Err(crate::errors::DEBUG_SPECULATIVE_ONLY.error(cursor));
        }
        for id in run_state.speculative_subtree(cursor) {
            run_state.retire_cursor(id);
        }
        self.db
            .update_workflow_run_status(
                run.id,
                run.status,
                run.active_node_id.clone(),
                Some(run_state),
                Some("Speculative branch retired".into()),
            )
            .await?;
        Ok(TaskResponse {
            success: true,
            message: format!("Speculative branch {cursor} retired"),
        })
    }

    /// let a speculative cursor dispatch one node for real instead of shadowing it.
    async fn arm_node(
        &self,
        cursor: Uuid,
        node_id: String,
        armed: bool,
    ) -> Result<TaskResponse, SendableError> {
        let run = &self.run;
        let mut run_state = run.execution_state.clone();
        if !run_state.is_speculative(cursor) {
            return Err(crate::errors::DEBUG_SPECULATIVE_ONLY.error(cursor));
        }
        let Some(frame) = run_state
            .cursor_mut(cursor)
            .and_then(|cursor| cursor.speculative.as_mut())
        else {
            return Err(crate::errors::DEBUG_CURSOR_NOT_FOUND.error(cursor));
        };
        if armed {
            frame.armed_nodes.insert(node_id.clone());
        } else {
            frame.armed_nodes.remove(&node_id);
        }
        self.db
            .update_workflow_run_status(
                run.id,
                run.status,
                run.active_node_id.clone(),
                Some(run_state),
                None,
            )
            .await?;
        Ok(TaskResponse {
            success: true,
            message: format!(
                "{node_id} {} for real execution",
                if armed { "armed" } else { "disarmed" }
            ),
        })
    }

    /// apply `mutate` to one cursor's debugger runtime, mirror the primary, persist, and re-arm a ready
    /// node for that cursor so the reducer actually picks the thread of control back up.
    ///
    /// the re-arm is the half that never existed: the debug endpoints wrote intent and enqueued nothing,
    /// so even once the reducer learned to read the frame there was nothing to wake it.
    async fn persist_cursor_debug(
        &self,
        requested: Option<Uuid>,
        message: Option<String>,
        mutate: impl FnOnce(&mut runinator_models::workflow_state::DebugRuntime),
    ) -> Result<Uuid, SendableError> {
        let run = &self.run;
        let mut run_state = run.execution_state.clone();
        run_state
            .debug
            .get_or_insert_with(DebugFrame::default)
            .config
            .enabled = true;
        let cursor_id = resolve_target_cursor(&run_state, requested)?;
        let mut runtime = run_state.cursor_debug(cursor_id);
        mutate(&mut runtime);
        run_state.set_cursor_debug(cursor_id, runtime);
        let node_id = run_state
            .cursor(cursor_id)
            .map(|cursor| cursor.node_id().to_string());

        self.db
            .update_workflow_run_status(
                run.id,
                WorkflowStatus::Running,
                run.active_node_id.clone(),
                Some(run_state),
                message,
            )
            .await?;
        if let Some(node_id) = node_id {
            support::enqueue_node_ready_for_cursor(
                self.db,
                run.id,
                cursor_id,
                node_id,
                "debug_resume",
                Utc::now(),
            )
            .await?;
        }
        Ok(cursor_id)
    }

    /// load the typed run state, apply `mutate` to the debug frame (always marking it enabled), and
    /// persist with the given status/message. the single typed write path for debug bookkeeping.
    async fn persist_debug_frame(
        &self,
        status: WorkflowStatus,
        message: Option<String>,
        mutate: impl FnOnce(&mut DebugFrame),
    ) -> Result<(), SendableError> {
        let run = &self.run;
        let mut run_state = run.execution_state.clone();
        let frame = run_state.debug.get_or_insert_with(DebugFrame::default);
        frame.config.enabled = true;
        mutate(frame);
        self.db
            .update_workflow_run_status(
                run.id,
                status,
                run.active_node_id.clone(),
                Some(run_state),
                message,
            )
            .await
    }

    /// advance one thread of control by exactly one node.
    async fn step_cursor(&self, cursor: Option<Uuid>) -> Result<TaskResponse, SendableError> {
        self.persist_cursor_debug(cursor, Some("Debug step requested".into()), |runtime| {
            runtime.paused = false;
            runtime.step_requested = true;
        })
        .await?;
        Ok(TaskResponse {
            success: true,
            message: "Debug step requested".into(),
        })
    }

    /// resume one thread of control, still honoring breakpoints.
    async fn continue_cursor(&self, cursor: Option<Uuid>) -> Result<TaskResponse, SendableError> {
        self.persist_cursor_debug(cursor, Some("Debug continue requested".into()), |runtime| {
            runtime.paused = false;
            runtime.step_requested = false;
        })
        .await?;
        Ok(TaskResponse {
            success: true,
            message: "Debug continue requested".into(),
        })
    }

    async fn set_breakpoints(
        &self,
        breakpoints: Vec<String>,
    ) -> Result<TaskResponse, SendableError> {
        let status = self.run.status;
        self.persist_debug_frame(status, None, |frame| {
            frame.config.breakpoints = breakpoints;
        })
        .await?;
        Ok(TaskResponse {
            success: true,
            message: "Breakpoints updated".into(),
        })
    }

    async fn set_mode(&self, mode: DebugMode) -> Result<TaskResponse, SendableError> {
        let status = self.run.status;
        self.persist_debug_frame(status, None, |frame| {
            frame.config.mode = Some(mode);
        })
        .await?;
        Ok(TaskResponse {
            success: true,
            message: "Debug mode updated".into(),
        })
    }

    async fn update_debug(&self, patch: Value) -> Result<TaskResponse, SendableError> {
        let invalid = |detail: &str| crate::errors::DEBUG_INVALID_PATCH.error(detail);
        let patch_obj = patch
            .as_object()
            .ok_or_else(|| invalid("Debug patch must be a JSON object"))?;

        // validate the whole patch before touching state.
        let breakpoints = match patch_obj.get("breakpoints") {
            Some(bps) => Some(
                bps.as_array()
                    .ok_or_else(|| invalid("breakpoints must be an array of node ids"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| invalid("breakpoints must be an array of node ids"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        let mode = match patch_obj.get("mode") {
            Some(m) => Some(
                serde_json::from_value::<DebugMode>(m.clone().into())
                    .map_err(|_| invalid("mode must be 'step_all' or 'breakpoints'"))?,
            ),
            None => None,
        };
        let one_shot = match patch_obj.get("one_shot_breakpoint") {
            Some(Value::Null) => Some(None),
            Some(osb) => Some(Some(
                osb.as_str()
                    .ok_or_else(|| invalid("one_shot_breakpoint must be a node id string or null"))?
                    .to_string(),
            )),
            None => None,
        };

        let status = self.run.status;
        self.persist_debug_frame(status, None, |frame| {
            if let Some(breakpoints) = breakpoints {
                frame.config.breakpoints = breakpoints;
            }
            if let Some(mode) = mode {
                frame.config.mode = Some(mode);
            }
            if let Some(one_shot) = one_shot {
                frame.runtime.one_shot_breakpoint = one_shot;
            }
        })
        .await?;
        Ok(TaskResponse {
            success: true,
            message: "Debug settings updated".into(),
        })
    }
}

pub async fn pause_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Pause);
    pause_workflow_run_command(db, &command).await
}

async fn pause_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::PAUSE_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = run.execution_state.clone();
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = true;

    let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let has_running_node = run
        .active_node_id
        .as_deref()
        .and_then(|node_id| latest_node_run_for(&node_runs, node_id))
        .is_some_and(|node_run| node_run.status == WorkflowStatus::Running);
    let debug_enabled = run_state
        .debug
        .as_ref()
        .map(|debug| debug.config.enabled)
        .unwrap_or(false);
    let status = if has_running_node || debug_enabled {
        run.status
    } else {
        WorkflowStatus::Paused
    };

    db.update_workflow_run_status(
        workflow_run_id,
        status,
        run.active_node_id,
        Some(run_state),
        Some("Workflow pause requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} pause requested"),
    })
}

pub async fn resume_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Resume);
    resume_workflow_run_command(db, &command).await
}

async fn resume_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::RESUME_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = run.execution_state.clone();
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = false;
    let status = if matches!(
        run.status,
        WorkflowStatus::Paused | WorkflowStatus::DebugPaused
    ) {
        WorkflowStatus::Running
    } else {
        run.status
    };
    if run.status == WorkflowStatus::DebugPaused {
        release_every_cursor(&mut run_state);
    }

    db.update_workflow_run_status(
        workflow_run_id,
        status,
        run.active_node_id,
        Some(run_state),
        Some("Workflow resume requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} resumed"),
    })
}

pub async fn cancel_workflow_run<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Cancel);
    let response = cancel_workflow_run_command(db, &command).await?;
    // an invocation's in-flight calls are not node runs, so settling the run's node runs leaves
    // them open. left `Running` they would keep reporting as pending work for a run that is over,
    // and a late worker result would resume a program whose run is already terminal.
    if let Err(err) = db
        .cancel_invocation_calls_for_run(workflow_run_id, "workflow run canceled")
        .await
    {
        log::warn!("Failed to cancel invocation calls for run {workflow_run_id}: {err}");
    }
    if let Err(err) = cancel_workflow_task_runs(db, workflow_run_id).await {
        log::warn!("Failed to cancel provider tasks for run {workflow_run_id}: {err}");
    }
    // reliable path first: a cancel routed to the replica holding each in-flight action's executor
    // lease, so it cannot be consumed (and dropped) by a worker holding nothing. the untargeted
    // run-wide command below stays as a best-effort catch-all for executions whose claim was never
    // recorded (e.g. a fail-open executor claim during a transient ws outage).
    publish_targeted_run_cancels(db, broker, workflow_run_id).await;
    publish_worker_control_command(broker, command).await?;
    Ok(response)
}

async fn cancel_workflow_task_runs<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<(), SendableError> {
    for task in db.fetch_workflow_task_runs(workflow_run_id).await? {
        if !task.status.is_terminal() {
            db.update_workflow_task_run(
                task.id,
                WorkflowStatus::Canceled,
                None,
                None,
                Some("workflow run canceled".into()),
            )
            .await?;
        }
    }
    Ok(())
}

/// tell the workers holding a run's in-flight actions that it is over, for a run whose terminal
/// state was already settled durably elsewhere (a `cancel_previous` schedule policy, say). same
/// two-step fan-out as [`cancel_workflow_run`], minus the state write.
pub async fn publish_run_cancel_commands<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) {
    publish_targeted_run_cancels(db, broker, workflow_run_id).await;
    let command = ControlCommand::new(workflow_run_id, ControlKind::Cancel);
    if let Err(err) = publish_worker_control_command(broker, command).await {
        log::warn!("Failed to publish run-wide cancel for run {workflow_run_id}: {err}");
    }
}

/// publish a node-run cancel targeted at the executor-holding replica for every node run of this
/// run still claimed by a worker. best-effort: a publish failure falls back to the untargeted
/// run-wide cancel and, past that, the node's own timeout backstop.
async fn publish_targeted_run_cancels<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) {
    let node_runs = match db.fetch_workflow_node_runs(workflow_run_id).await {
        Ok(node_runs) => node_runs,
        Err(err) => {
            log::warn!(
                "Failed to load node runs for targeted cancel fan-out of run {workflow_run_id}: {err}"
            );
            return;
        }
    };
    for node_run in node_runs {
        let Some(executor_replica_id) = node_run.current_executor_replica_id else {
            continue;
        };
        if node_run.status.is_terminal() {
            continue;
        }
        let command =
            ControlCommand::for_node_run(workflow_run_id, node_run.id, ControlKind::Cancel)
                .targeting_replica(executor_replica_id);
        if let Err(err) = broker.publish_control(command).await {
            log::warn!(
                "Failed to publish targeted cancel for node run {} of run {workflow_run_id}: {err}",
                node_run.id
            );
        }
    }
}

async fn cancel_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::CANCEL_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = run.execution_state.clone();
    // clear paused / step_requested so any in-flight scheduler tick sees the cancel.
    if let Some(debug) = run_state.debug.as_mut() {
        debug.runtime.paused = false;
        debug.runtime.step_requested = false;
    }
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = false;
    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Canceled,
        run.active_node_id,
        Some(run_state),
        Some("Workflow run canceled".into()),
    )
    .await?;
    super::release_run_mutexes(db, workflow_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} canceled"),
    })
}

async fn publish_worker_control_command(
    broker: &dyn Broker,
    command: ControlCommand,
) -> Result<(), SendableError> {
    broker
        .publish_control(command)
        .await
        .map_err(|err| crate::errors::CONTROL_PUBLISH.error(err))
}

impl<'a, T: DatabaseImpl> DebugSession<'a, T> {
    async fn run_to_cursor(
        &self,
        node_id: String,
        cursor: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let target = node_id.clone();
        // the one-shot is per-cursor: "run until here" is a question about one thread of control,
        // even though the breakpoint *set* it rides alongside is shared by all of them.
        self.persist_cursor_debug(
            cursor,
            Some(format!("Run to cursor at {node_id}")),
            move |runtime| {
                runtime.paused = false;
                runtime.step_requested = false;
                runtime.one_shot_breakpoint = Some(target);
            },
        )
        .await?;
        Ok(TaskResponse {
            success: true,
            message: format!("Running to cursor {}", node_id),
        })
    }

    async fn skip_node(
        &self,
        output_json: Value,
        message: Option<String>,
        _cursor: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let run = &self.run;
        let active_node_id = run
            .active_node_id
            .clone()
            .ok_or_else(|| crate::errors::DEBUG_NO_ACTIVE_NODE.error("no node to skip"))?;
        let nodes = self.db.fetch_workflow_node_runs(run.id).await?;
        let latest_node_run = nodes
            .into_iter()
            .filter(|n| n.node_id == active_node_id)
            .max_by_key(|n| n.attempt);
        let node_run = match latest_node_run {
            Some(n) => n,
            None => {
                self.db
                    .create_workflow_node_run(
                        run.id,
                        active_node_id.clone(),
                        Value::Null,
                        None,
                        None,
                    )
                    .await?
            }
        };
        let skip_message = message.clone().unwrap_or_else(|| "Skipped in debug".into());
        self.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Succeeded,
                None,
                None,
                Some(output_json),
                None,
                Some(DEBUG_SKIPPED.into()),
                Some(skip_message),
            )
            .await?;

        self.persist_debug_frame(
            WorkflowStatus::Running,
            Some(format!("Skipped node {}", active_node_id)),
            |frame| {
                frame.runtime.paused = false;
                frame.runtime.step_requested = true;
            },
        )
        .await?;
        Ok(TaskResponse {
            success: true,
            message: format!("Skipped node {}", active_node_id),
        })
    }

    async fn rerun_node(
        &self,
        parameters: Value,
        _cursor: Option<Uuid>,
    ) -> Result<TaskResponse, SendableError> {
        let run = &self.run;
        let active_node_id = run
            .active_node_id
            .clone()
            .ok_or_else(|| crate::errors::DEBUG_NO_ACTIVE_NODE.error("no node to re-run"))?;
        let nodes = self.db.fetch_workflow_node_runs(run.id).await?;
        let latest_node_run = nodes
            .into_iter()
            .filter(|n| n.node_id == active_node_id)
            .max_by_key(|n| n.attempt);
        let next_attempt = latest_node_run.as_ref().map(|r| r.attempt + 1).unwrap_or(1);
        if let Some(prior) = latest_node_run {
            self.db
                .update_workflow_node_run(
                    prior.id,
                    WorkflowStatus::Failed,
                    None,
                    None,
                    None,
                    None,
                    Some(DEBUG_SUPERSEDED.into()),
                    Some("Superseded by debug re-run".into()),
                )
                .await?;
        }
        let new_run = self
            .db
            .create_workflow_node_run(run.id, active_node_id.clone(), parameters, None, None)
            .await?;
        self.db
            .update_workflow_node_run(
                new_run.id,
                WorkflowStatus::Queued,
                Some(next_attempt),
                None,
                None,
                None,
                Some(DEBUG_RERUN.into()),
                None,
            )
            .await?;

        self.persist_debug_frame(
            WorkflowStatus::Running,
            Some(format!("Re-running node {}", active_node_id)),
            |frame| {
                frame.runtime.paused = false;
                frame.runtime.step_requested = true;
            },
        )
        .await?;
        Ok(TaskResponse {
            success: true,
            message: format!("Re-running node {}", active_node_id),
        })
    }
}

/// dispatch a single canonical [`DebugVerb`] against a run. every debug operation funnels through
/// here so the per-verb behavior lives in exactly one place.
pub async fn apply_debug_command<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    verb: DebugVerb,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .apply(verb)
        .await
}

/// fork a speculative "what if" branch beside the real ones.
pub async fn fork_debug_cursor<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    from_cursor: Option<Uuid>,
    at_node: Option<String>,
    label: Option<String>,
    context_patch: Value,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .fork_cursor(from_cursor, at_node, label, context_patch)
        .await
}

/// abandon a speculative branch and everything forked from it.
pub async fn retire_debug_cursor<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Uuid,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .retire_cursor(cursor)
        .await
}

/// let a speculative cursor dispatch one node for real instead of shadowing it.
pub async fn arm_debug_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Uuid,
    node_id: String,
    armed: bool,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .arm_node(cursor, node_id, armed)
        .await
}

/// advance one thread of control by exactly one node.
pub async fn step_debug_cursor<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .step_cursor(cursor)
        .await
}

/// resume one thread of control, still honoring breakpoints.
pub async fn continue_debug_cursor<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .continue_cursor(cursor)
        .await
}

pub async fn set_debug_breakpoints<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    breakpoints: Vec<String>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .set_breakpoints(breakpoints)
        .await
}

pub async fn set_debug_mode<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    mode: DebugMode,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .set_mode(mode)
        .await
}

pub async fn update_workflow_run_debug<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    patch: Value,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .update_debug(patch)
        .await
}

pub async fn run_to_cursor_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .run_to_cursor(node_id, cursor)
        .await
}

pub async fn skip_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    output_json: Value,
    message: Option<String>,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .skip_node(output_json, message, cursor)
        .await
}

pub async fn rerun_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    parameters: Value,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    DebugSession::load(db, workflow_run_id)
        .await?
        .rerun_node(parameters, cursor)
        .await
}

pub async fn replay_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    from_step_id: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    let Some(source) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::REPLAY_NOT_FOUND.error(workflow_run_id));
    };
    let snapshot = match source.workflow_snapshot.clone() {
        Some(snap) => snap,
        None => support::fetch_workflow_snapshot(db, source.workflow_id).await?,
    };

    let mut state = runinator_models::json!({
        "control": { "pause_requested": false },
        "debug": {
            "enabled": true,
            "paused": false,
            "step_requested": false,
            "mode": "breakpoints",
            "breakpoints": [],
            "one_shot_breakpoint": null
        },
        "replay": { "source_run_id": source.id }
    });

    // phase d: support resuming from a specific step.
    if let Some(target_node_id) = from_step_id.as_deref() {
        let ancestor_ids = ancestors_in_snapshot(&snapshot, target_node_id)?;
        if let Some(replay) = state.get_mut("replay").and_then(Value::as_object_mut) {
            replay.insert(
                "from_step_id".to_string(),
                Value::String(target_node_id.into()),
            );
        }
        let new_run = db
            .create_workflow_run(
                source.workflow_id,
                snapshot.clone(),
                source.parameters.clone(),
                state,
                source.name.clone(),
                runinator_models::replicas::WorkflowRunProvenance {
                    source_kind: Some(runinator_models::replicas::TriggerSourceKind::Replay),
                    actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                    actor_replica_id: None,
                    actor_display_name: Some("replay".into()),
                    request_host: None,
                    request_ip: None,
                    metadata: runinator_models::json!({ "source_run_id": source.id }),
                },
            )
            .await?;

        if !ancestor_ids.is_empty() {
            let source_nodes = db.fetch_workflow_node_runs(source.id).await?;
            for node_id in &ancestor_ids {
                if let Some(source_node) = source_nodes
                    .iter()
                    .rev()
                    .find(|node| node.node_id == *node_id && node.status.is_terminal())
                {
                    let new_node = db
                        .create_workflow_node_run(
                            new_run.id,
                            node_id.clone(),
                            source_node.parameters.clone(),
                            None,
                            // copied ancestor state, not a step any cursor took: the replay run
                            // seeds its own cursor on its first drive.
                            None,
                        )
                        .await?;
                    let attempt = if source_node.attempt > 0 {
                        Some(source_node.attempt)
                    } else {
                        Some(1)
                    };
                    db.update_workflow_node_run(
                        new_node.id,
                        source_node.status,
                        attempt,
                        None,
                        source_node.output_json.clone(),
                        Some(source_node.state.clone()),
                        Some("replayed_from_source".into()),
                        Some(format!("Replayed from run {} step {}", source.id, node_id)),
                    )
                    .await?;
                }
            }
        }
        db.update_workflow_run_status(
            new_run.id,
            WorkflowStatus::Queued,
            Some(target_node_id.to_string()),
            None,
            Some(format!(
                "Replayed from run {} starting at step {}",
                source.id, target_node_id
            )),
        )
        .await?;
        support::enqueue_node_ready(
            db,
            new_run.id,
            target_node_id.to_string(),
            "workflow_run_replay",
            Utc::now(),
            runinator_models::json!({ "node_id": target_node_id }),
        )
        .await?;

        let Some(refreshed) = db.fetch_workflow_run(new_run.id).await? else {
            return Err(crate::errors::REPLAY_NOT_FOUND
                .error(format!("replay run {} disappeared", new_run.id)));
        };
        return Ok(refreshed);
    }

    let run = db
        .create_workflow_run(
            source.workflow_id,
            snapshot,
            source.parameters,
            state,
            source.name,
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Replay),
                actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("replay".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({ "source_run_id": source.id }),
            },
        )
        .await?;
    support::enqueue_start_ready_node(db, &run).await?;
    Ok(run)
}

/// BFS over reverse transitions from `target_node_id` to find all nodes that must
/// have completed before the target can run. Refuses to traverse through
/// `Loop`/`Map`/`Parallel`/`Try` ancestors — multi-iteration state can't be
/// safely copied in v1 (Phase D limitation).
pub fn ancestors_in_snapshot(
    snapshot: &WorkflowDefinition,
    target_node_id: &str,
) -> Result<Vec<String>, SendableError> {
    use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    let nodes: Vec<WorkflowNode> = snapshot.definition.nodes.clone();

    if nodes.is_empty() {
        return Ok(Vec::new());
    }

    if !nodes.iter().any(|node| node.id == target_node_id) {
        return Err(crate::errors::REPLAY_MISSING_STEP.error(target_node_id));
    }

    // build reverse adjacency: for each node, the set of nodes that transition into it.
    let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let by_id: BTreeMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    for node in &nodes {
        for child_id in transition_targets(node) {
            reverse.entry(child_id).or_default().insert(node.id.clone());
        }
    }

    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    if let Some(parents) = reverse.get(target_node_id) {
        for parent in parents {
            queue.push_back(parent.clone());
        }
    }

    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        if let Some(node) = by_id.get(node_id.as_str())
            && matches!(
                node.kind,
                WorkflowNodeKind::Loop
                    | WorkflowNodeKind::Map
                    | WorkflowNodeKind::Parallel
                    | WorkflowNodeKind::Try
                    | WorkflowNodeKind::Race
            )
        {
            return Err(crate::errors::REPLAY_CONTROL_FLOW.error(format!(
                "cannot restart from step {target_node_id}: ancestor {node_id} is a control-flow node ({:?}) whose state is not safely replayable",
                node.kind
            )));
        }
        if let Some(parents) = reverse.get(&node_id) {
            for parent in parents {
                queue.push_back(parent.clone());
            }
        }
    }

    // topologically sort the ancestor set so each node only depends on earlier-seeded outputs.
    let mut order = Vec::new();
    let mut remaining: BTreeSet<String> = visited.clone();
    while !remaining.is_empty() {
        // pick any node in `remaining` whose ancestors are all already placed.
        let next = remaining
            .iter()
            .find(|node_id| {
                reverse
                    .get(*node_id)
                    .map(|parents| parents.iter().all(|parent| !remaining.contains(parent)))
                    .unwrap_or(true)
            })
            .cloned();
        if let Some(node_id) = next {
            remaining.remove(&node_id);
            order.push(node_id);
        } else {
            // fallback: cycle detected; fall back to insertion order.
            order.extend(remaining.iter().cloned());
            remaining.clear();
        }
    }
    Ok(order)
}

fn transition_targets(node: &runinator_models::workflows::WorkflowNode) -> Vec<String> {
    use runinator_models::value::Value;
    let mut targets = Vec::new();
    fn walk(value: &Value, into: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(target) = map.get("$node").and_then(|value| value.as_str()) {
                    into.push(target.to_string());
                    return;
                }
                for value in map.values() {
                    walk(value, into);
                }
            }
            Value::Array(items) => {
                for value in items {
                    walk(value, into);
                }
            }
            _ => {}
        }
    }
    let transitions_value = serde_json::to_value(&node.transitions)
        .map(Value::from)
        .unwrap_or(Value::Null);
    walk(&transitions_value, &mut targets);
    let condition_value = node.condition.to_value();
    walk(&condition_value, &mut targets);
    walk(&node.parameters, &mut targets);
    targets
}

async fn require_debug_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<WorkflowRun, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::DEBUG_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Err(crate::errors::DEBUG_TERMINAL.error(workflow_run_id));
    }
    if !run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(crate::errors::DEBUG_DISABLED.error(workflow_run_id));
    }
    Ok(run)
}
