// control-flow nodes drive nested node graphs by keeping bookkeeping in named `RunState` frames
// (loop/parallel/map/race/try) and routing the workflow into and out of those subgraphs. they
// override `process` because their re-entrancy is bespoke rather than the generic poller shape.

use async_trait::async_trait;
use runinator_comm::WireCodec;
use runinator_models::{
    errors::SendableError,
    workflow_state::{
        LoopFrame, LoopOutput, MapFrame, MapOutput, ParallelFrame, ParallelOutput, RaceFrame,
        RaceOutput, TryFrame,
    },
    workflows::{WorkflowNodeKind, WorkflowNodeRun, WorkflowStatus},
};

use crate::nodes::context::NodeContext;
use crate::nodes::driver;
use crate::nodes::handler::{NodeHandler, NodeOutcome};
use crate::nodes::run_state::{RunState, append_completed_map_item, latest_status, race_winner};

pub struct LoopHandler;

#[async_trait]
impl NodeHandler for LoopHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Loop
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let context = ctx.runtime_context();
        let parameters = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let items = runinator_workflows::parse_loop_items(&parameters).items;
        let prior_iterations = ctx
            .node_runs
            .iter()
            .filter(|run| run.node_id == ctx.node.id && run.status == WorkflowStatus::Succeeded)
            .count() as i64;
        let max_iterations = ctx.node.max_iterations.unwrap_or(i64::MAX).max(0);
        let index = prior_iterations;
        let exhausted = index >= items.len() as i64 || index >= max_iterations;
        // each iteration gets its own run so prior_iterations advances. reuse the latest only if it
        // was left running from a prior interrupted visit.
        let node_run = match ctx.latest_with_status(WorkflowStatus::Running) {
            Some(latest) => {
                if ctx.timed_out(latest) {
                    return ctx.time_out(latest, "Loop node timed out").await;
                }
                latest.clone()
            }
            None => ctx.create_node_run_with(parameters.clone()).await?,
        };
        let output = if exhausted {
            LoopOutput {
                index,
                item: None,
                has_next: false,
                count: items.len(),
            }
        } else {
            LoopOutput {
                index,
                item: Some(items[index as usize].clone()),
                has_next: true,
                count: items.len(),
            }
        };
        let output_value = output.to_wire_value()?;
        let reason = if exhausted {
            "loop_exhausted"
        } else {
            "loop_iteration"
        };
        // mark the iteration succeeded so prior_iterations advances on re-entry from the loop body.
        // without this the loop would re-process index 0 forever.
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            Some(output_value.clone()),
            None,
            Some(reason.into()),
            None,
        )
        .await?;

        if exhausted {
            // clear loop bookkeeping before exiting. otherwise the last iteration's loop frame
            // survives into the exit path and the first downstream end node would route back into
            // the loop.
            let mut state = ctx.run_state();
            state.clear_loop();
            ctx.update_run(
                ctx.workflow_run.status,
                ctx.workflow_run.active_node_id.clone(),
                Some(state.into_value()?),
                None,
            )
            .await?;
            return ctx
                .transition(
                    &node_run,
                    WorkflowStatus::Succeeded,
                    Some(output_value),
                    Some("loop_exhausted".into()),
                )
                .await;
        }

        let return_to = ctx
            .node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string())
            .unwrap_or_else(|| ctx.node.id.clone());
        let mut state = RunState::default();
        state.set_loop(LoopFrame {
            index,
            item: items[index as usize].clone(),
            return_to: ctx.node.id.clone(),
        });
        ctx.update_run(
            WorkflowStatus::Running,
            Some(return_to.clone()),
            Some(state.into_value()?),
            None,
        )
        .await?;
        Ok(NodeOutcome::Advanced {
            status: WorkflowStatus::Running,
            target: Some(return_to),
        })
    }
}

pub struct ParallelHandler;

#[async_trait]
impl NodeHandler for ParallelHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Parallel
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        if let Some(node_run) = ctx.latest {
            if node_run.status == WorkflowStatus::Running && ctx.timed_out(node_run) {
                return ctx.time_out(node_run, "Parallel node timed out").await;
            }
            return Ok(NodeOutcome::Pending);
        }
        let params = runinator_workflows::parse_parallel_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let Some(first) = params.branches.first().cloned() else {
            return ctx.block("Parallel node has no branches").await;
        };
        let branches = params
            .branches
            .iter()
            .map(|branch| branch.as_str().to_string())
            .collect::<Vec<_>>();
        let remaining = branches.iter().skip(1).cloned().collect::<Vec<_>>();
        let node_run = ctx.create_node_run().await?;
        let output = ParallelOutput { branches };
        let mut state = ctx.run_state();
        state.set_parallel(ParallelFrame {
            node_id: ctx.node.id.clone(),
            remaining,
        });
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            Some(output.to_wire_value()?),
            None,
            Some("parallel_started".into()),
            None,
        )
        .await?;
        ctx.goto(first.into_string(), Some(state.into_value()?), None)
            .await
    }
}

pub struct MapHandler;

#[async_trait]
impl NodeHandler for MapHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Map
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let params = runinator_workflows::parse_map_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ctx.ensure_node_run().await?;
        let mut frame = if ctx
            .run_state()
            .map()
            .is_some_and(|frame| frame.node_id == ctx.node.id)
        {
            // re-entry: confirm the dispatched item succeeded, then record its output.
            let existing = ctx.run_state().map().cloned().unwrap_or_else(|| MapFrame {
                node_id: ctx.node.id.clone(),
                target: params.target.as_str().to_string(),
                items: Vec::new(),
                index: 0,
                outputs: Vec::new(),
                concurrency: params.concurrency.unwrap_or(1),
                item: None,
            });
            if let Some(status) = latest_status(params.target.as_str(), ctx.node_runs)
                && status != WorkflowStatus::Succeeded
            {
                return ctx
                    .transition(&node_run, status, None, Some("map_item_failed".into()))
                    .await;
            }
            append_completed_map_item(existing, params.target.as_str(), ctx.node_runs)
        } else {
            // first visit: resolve the item list and initialize the frame.
            let context = ctx.runtime_context();
            let items = runinator_workflows::resolve_value_refs(&params.items, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let items = items.as_array().cloned().unwrap_or_default();
            MapFrame {
                node_id: ctx.node.id.clone(),
                target: params.target.as_str().to_string(),
                items,
                index: 0,
                outputs: Vec::new(),
                concurrency: params.concurrency.unwrap_or(1),
                item: None,
            }
        };
        if frame.index >= frame.items.len() as i64 {
            let output = MapOutput {
                count: frame.items.len(),
                outputs: frame.outputs.clone(),
            };
            return ctx
                .transition(
                    &node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("map_exhausted".into()),
                )
                .await;
        }
        frame.item = Some(frame.items[frame.index as usize].clone());
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            Some(frame.to_wire_value()?),
            Some("map_iteration".into()),
            None,
        )
        .await?;
        let mut state = ctx.run_state();
        state.set_map(frame);
        ctx.goto(params.target.into_string(), Some(state.into_value()?), None)
            .await
    }
}

pub struct RaceHandler;

#[async_trait]
impl NodeHandler for RaceHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Race
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let params = runinator_workflows::parse_race_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ctx.ensure_node_run().await?;
        if node_run.status == WorkflowStatus::Running && ctx.timed_out(&node_run) {
            return ctx.time_out(&node_run, "Race node timed out").await;
        }
        let branches = params
            .branches
            .iter()
            .map(|branch| branch.as_str().to_string())
            .collect::<Vec<_>>();
        if let Some(winner) = race_winner(&branches, params.winner, ctx.node_runs) {
            let output = RaceOutput { winner };
            return ctx
                .transition(
                    &node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("race_won".into()),
                )
                .await;
        }
        let mut state = ctx.run_state();
        let next_target = if state.race_owned_by(&ctx.node.id) {
            state.pop_race_remaining()
        } else {
            let remaining = branches.iter().skip(1).cloned().collect::<Vec<_>>();
            state.set_race(RaceFrame {
                node_id: ctx.node.id.clone(),
                remaining,
            });
            branches.first().cloned()
        };
        if let Some(target) = next_target {
            ctx.update_node_run(
                node_run.id,
                WorkflowStatus::Running,
                Some(node_run.attempt + 1),
                None,
                None,
                None,
                Some("race_branch_started".into()),
                None,
            )
            .await?;
            return ctx.goto(target, Some(state.into_value()?), None).await;
        }
        ctx.transition(
            &node_run,
            WorkflowStatus::Failed,
            None,
            Some("Race completed without a winning branch".into()),
        )
        .await
    }
}

pub struct TryHandler;

#[async_trait]
impl NodeHandler for TryHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Try
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let params = runinator_workflows::parse_try_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ctx.ensure_node_run().await?;
        if node_run.status == WorkflowStatus::Running && ctx.timed_out(&node_run) {
            return ctx.time_out(&node_run, "Try node timed out").await;
        }
        let frame = ctx
            .run_state()
            .try_frame()
            .cloned()
            .unwrap_or_else(|| TryFrame {
                node_id: ctx.node.id.clone(),
                phase: "body".into(),
                pending_status: None,
            });
        let phase = frame.phase.clone();
        if ctx.latest.is_none() {
            return start_phase(ctx, &node_run, params.body.as_str(), "body", None).await;
        }
        match phase.as_str() {
            "body" => {
                let Some(status) = latest_status(params.body.as_str(), ctx.node_runs) else {
                    return Ok(NodeOutcome::Pending);
                };
                if status == WorkflowStatus::Succeeded {
                    if let Some(finally) = params.finally {
                        return start_phase(
                            ctx,
                            &node_run,
                            finally.as_str(),
                            "finally",
                            Some(status),
                        )
                        .await;
                    }
                    return ctx
                        .transition(&node_run, status, None, Some("try_body_succeeded".into()))
                        .await;
                }
                if let Some(catch) = params.catch {
                    return start_phase(ctx, &node_run, catch.as_str(), "catch", Some(status))
                        .await;
                }
                if let Some(finally) = params.finally {
                    return start_phase(ctx, &node_run, finally.as_str(), "finally", Some(status))
                        .await;
                }
                ctx.transition(&node_run, status, None, Some("try_body_failed".into()))
                    .await
            }
            "catch" => {
                let Some(status) = params
                    .catch
                    .as_ref()
                    .and_then(|catch| latest_status(catch.as_str(), ctx.node_runs))
                else {
                    return Ok(NodeOutcome::Pending);
                };
                if let Some(finally) = params.finally {
                    return start_phase(ctx, &node_run, finally.as_str(), "finally", Some(status))
                        .await;
                }
                ctx.transition(&node_run, status, None, Some("try_catch_completed".into()))
                    .await
            }
            "finally" => {
                let Some(finally) = params.finally.as_ref().map(|target| target.as_str()) else {
                    return Ok(NodeOutcome::Pending);
                };
                if latest_status(finally, ctx.node_runs).is_none() {
                    return Ok(NodeOutcome::Pending);
                }
                let status = frame.pending_status.unwrap_or(WorkflowStatus::Succeeded);
                ctx.transition(
                    &node_run,
                    status,
                    None,
                    Some("try_finally_completed".into()),
                )
                .await
            }
            _ => ctx.block("Try node has invalid phase").await,
        }
    }
}

/// advance a try node into a phase and report the jump.
async fn start_phase(
    ctx: &NodeContext<'_>,
    node_run: &WorkflowNodeRun,
    target: &str,
    phase: &str,
    pending_status: Option<WorkflowStatus>,
) -> Result<NodeOutcome, SendableError> {
    driver::start_try_phase(
        ctx.api,
        ctx.workflow_run,
        node_run,
        ctx.node,
        target,
        phase,
        pending_status,
    )
    .await?;
    Ok(NodeOutcome::Advanced {
        status: WorkflowStatus::Running,
        target: Some(target.to_string()),
    })
}
