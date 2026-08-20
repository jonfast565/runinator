use super::context::runtime_context;
use super::transitions::{
    arm_node_timeout, ensure_node_run, time_out, timed_out, transition_from_node,
};
use super::*;

/// fan out a `map` node: keep up to `concurrency` child runs in flight, one per item, gather their
/// body outputs in item order, and fail fast if any item fails. each item runs the body subgraph as
/// an isolated child run (see [`create_map_child_run`]); the body returns to the map node, where the
/// engine stop-boundary finalizes the child and wakes this parent.
pub(super) struct MapOp;

impl MapOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let params = runinator_workflows::parse_map_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        if node_run.status == WorkflowStatus::Running && timed_out(ctx.timing(), &node_run) {
            return super::execution::complete(
                time_out(ctx, &node_run, "Map node timed out").await,
            );
        }

        let mut frame = match ctx.run_state_snapshot().map.clone() {
            Some(frame) if frame.node_id == ctx.node.id => frame,
            _ => {
                // first visit: resolve the item list and initialize the fan-out frame.
                let context = runtime_context(ctx).await;
                let items = runinator_workflows::evaluate_expression(&params.items, &context)
                    .map_err(|err| -> SendableError { Box::new(err) })?;
                let items = items.as_array().cloned().unwrap_or_default();
                MapFrame {
                    node_id: ctx.node.id.clone(),
                    target: params.target.as_str().to_string(),
                    concurrency: params.concurrency.unwrap_or(1).max(1),
                    next_index: 0,
                    in_flight: Vec::new(),
                    results: vec![Value::Null; items.len()],
                    done: 0,
                    items,
                    item: None,
                    index: 0,
                }
            }
        };
        let total = frame.items.len() as i64;

        // harvest finished children; bail to fail-fast on the first failed item.
        let children = std::mem::take(&mut frame.in_flight);
        let mut still_in_flight = Vec::with_capacity(children.len());
        let mut failure: Option<String> = None;
        for child in &children {
            let Some(child_run) = ctx.db.fetch_workflow_run(child.child_run_id).await? else {
                failure = Some(format!("Map child run {} is missing", child.child_run_id));
                break;
            };
            match child_run.status {
                WorkflowStatus::Succeeded => {
                    if let Some(slot) = frame.results.get_mut(child.index as usize) {
                        *slot = map_child_result(&child_run);
                    }
                    frame.done += 1;
                }
                status if status.is_terminal() => {
                    failure =
                        Some(child_run.message.unwrap_or_else(|| {
                            format!("Map item {} did not succeed", child.index)
                        }));
                    break;
                }
                _ => still_in_flight.push(child.clone()),
            }
        }

        if let Some(message) = failure {
            cancel_children(ctx, &children).await?;
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Failed,
                None,
                Some(format!("map_item_failed: {message}")),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        frame.in_flight = still_in_flight;

        // top up the window with new items.
        while (frame.in_flight.len() as i64) < frame.concurrency && frame.next_index < total {
            let index = frame.next_index;
            let item = frame.items[index as usize].clone();
            let child_run_id = create_map_child_run(ctx, &frame, index, item).await?;
            frame.in_flight.push(MapChild {
                index,
                child_run_id,
            });
            frame.next_index += 1;
        }

        // all items done: emit ordered outputs and continue.
        if frame.done >= total && frame.in_flight.is_empty() {
            let output = MapOutput {
                count: total as usize,
                outputs: frame.results.clone(),
            };
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("map_exhausted".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        // park the parent while children run; persist the fan-out frame.
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Running,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(frame.to_wire_value()?),
                Some("map_fanout".into()),
                None,
            )
            .await?;
        let mut state = ctx.run_state_snapshot().clone();
        state.map = Some(frame);
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                Some(state),
                None,
            )
            .await?;
        super::execution::complete(arm_node_timeout(ctx).await)
    }
}

/// finalize a map fan-out child when its body returns to the controlling map node. captures the
/// body output into `state.map_child.result`, marks the child `Succeeded`, and lets
/// `maybe_wake_subflow_parent` wake the parent map node. invoked from the engine stop-boundary.
pub(super) async fn finalize_map_child<T: RuntimeStore>(
    ctx: &super::execution::RunStepContext<'_, T>,
    child: MapChildState,
) -> Result<(), SendableError> {
    // body output is the latest terminal node-run other than the synthetic map binding.
    let output = ctx
        .node_runs
        .iter()
        .filter(|run| run.node_id != child.stop_node && run.status.is_terminal())
        .max_by_key(|run| run.id)
        .and_then(|run| run.output_json.clone())
        .unwrap_or(Value::Null);
    let mut typed = ctx.workflow_run.execution_state.clone();
    if let Some(child_state) = typed.map_child.as_mut() {
        child_state.result = Some(output);
    }
    // the child is finished, so it holds no threads of control. leaving them would make a terminal
    // run disagree with the invariant every other terminal path maintains.
    typed.cursors.clear();
    ctx.db
        .update_workflow_run_status(
            ctx.workflow_run.id,
            WorkflowStatus::Succeeded,
            ctx.workflow_run.active_node_id.clone(),
            Some(typed),
            Some("map_child_finished".into()),
        )
        .await
}

/// read the body output a finished map child stashed under `state.map_child.result`.
fn map_child_result(child_run: &WorkflowRun) -> Value {
    child_run
        .execution_state
        .map_child
        .as_ref()
        .and_then(|map_child| map_child.result.as_ref())
        .cloned()
        .unwrap_or(Value::Null)
}

/// create one child run that executes the map body bound to a single item. the child shares the
/// parent's workflow snapshot and run parameters, starts at the body entry, stops when it returns to
/// the map node, and is linked back to the parent via `state.subflow_parent` for wake-up.
async fn create_map_child_run<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    frame: &MapFrame,
    index: i64,
    item: Value,
) -> Result<Uuid, SendableError> {
    let parent_run = ctx.workflow_run;
    let map_node = ctx.node;
    let snapshot = match parent_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => ctx
            .db
            .fetch_workflow(parent_run.workflow_id)
            .await?
            .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(parent_run.workflow_id))?,
    };
    let target = frame.target.clone();
    let state = runinator_models::json!({
        "control": { "pause_requested": false },
        "subflow_parent": { "run_id": parent_run.id, "node_id": map_node.id },
        "map_child": { "stop_node": map_node.id, "index": index, "item": item },
        "map": {
            "node_id": map_node.id,
            "target": target,
            "item": item,
            "index": index,
            "concurrency": 1
        }
    });
    let child = ctx
        .db
        .create_workflow_run(
            parent_run.workflow_id,
            snapshot,
            parent_run.parameters.clone(),
            state,
            None,
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Map),
                actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("map".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({
                    "parent_run_id": parent_run.id,
                    "parent_node_id": map_node.id,
                    "index": index,
                }),
            },
        )
        .await?;
    // seed the map node's output so the body resolves the map variable (`node:<map>,output:[item]`).
    let seed = ctx
        .db
        .create_workflow_node_run(
            child.id,
            map_node.id.clone(),
            map_node.parameters.clone().into(),
            // the seed is the first node run in the child run; it has no in-run predecessor.
            None,
            // and no cursor: the parent's belongs to the parent run. the child seeds its own on
            // its first drive, so stamping this one would attribute the row to a foreign run.
            None,
        )
        .await?;
    ctx.db
        .update_workflow_node_run(
            seed.id,
            WorkflowStatus::Succeeded,
            Some(seed.attempt + 1),
            None,
            Some(runinator_models::json!({ "item": item, "index": index })),
            None,
            Some("map_item_bound".into()),
            None,
        )
        .await?;
    // drive the child from the body entry node.
    let event = NewOrchestrationEvent::new(
        child.id,
        Some(target.clone()),
        "map_child_created",
        runinator_models::json!({
            "parent_run_id": parent_run.id,
            "node_id": map_node.id,
            "index": index,
        }),
    );
    ctx.db.enqueue_ready_node(event, target, Utc::now()).await?;
    Ok(child.id)
}

/// cancel any map children that are still running (fail-fast on a sibling failure).
async fn cancel_children<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    children: &[MapChild],
) -> Result<(), SendableError> {
    for child in children {
        let Some(child_run) = ctx.db.fetch_workflow_run(child.child_run_id).await? else {
            continue;
        };
        if child_run.status.is_terminal() {
            continue;
        }
        ctx.db
            .update_workflow_run_status(
                child.child_run_id,
                WorkflowStatus::Canceled,
                child_run.active_node_id,
                None,
                Some("map_sibling_failed".into()),
            )
            .await?;
    }
    Ok(())
}
