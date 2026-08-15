use super::context::{is_reentry_stale, runtime_context};
use super::transitions::{time_out, transition_from_node};
use super::*;
use runinator_workflows::GateTimeoutPolicy;

/// process a gate node: an automated/policy block. on first visit it records a gate row and parks
/// the run as `Waiting`; on each poll wake it re-checks whether the gate is open (auto-evaluated for
/// `condition` gates, read from the gate row for `manual`/`external` gates) and transitions on pass,
/// times out at the optional deadline, or re-arms the next poll. mirrors `wait` (poll/re-arm) and
/// `approval` (park + record), but resolves by policy rather than by a human decision.
pub(super) struct GateHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for GateHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = runinator_workflows::parse_gate_parameters(ctx.node);
        // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a fresh
        // visit so a new gate is opened instead of transitioning from the stale run.
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
            let gate_state =
                serde_json::from_value::<GateState>(node_run.state.clone().into()).ok();
            let gate_id = gate_state.as_ref().and_then(|state| state.gate_id);

            // honor an explicit max-wait deadline before re-checking.
            if let Some(deadline) = gate_state.as_ref().and_then(|state| state.deadline_unix)
                && Utc::now().timestamp() >= deadline
            {
                if let Some(gate_id) = gate_id {
                    mark_gate(ctx, gate_id, "timed_out", None, None).await?;
                }
                match params.timeout_policy {
                    GateTimeoutPolicy::Fail => {
                        time_out(ctx, node_run, "Gate timed out").await?;
                    }
                    GateTimeoutPolicy::Continue => {
                        transition_from_node(
                            ctx,
                            node_run,
                            WorkflowStatus::Succeeded,
                            Some(runinator_models::json!({
                                "gate_passed": false,
                                "gate_timed_out": true,
                            })),
                            Some("gate_timeout_continued".into()),
                        )
                        .await?;
                    }
                }
                return Ok(ReadyNodeDisposition::Complete);
            }

            if gate_is_open(ctx, &params, gate_id).await? {
                if let Some(gate_id) = gate_id {
                    mark_gate(ctx, gate_id, "passed", None, None).await?;
                }
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(runinator_models::json!({ "gate_passed": true })),
                    Some("gate_passed".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }

            // still closed: re-arm the next poll and keep the claim.
            enqueue_gate_poll(ctx, params.poll_interval_seconds).await?;
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // first visit: record the gate row, park the node, schedule the first poll.
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let deadline_unix = params
            .deadline_seconds
            .map(|seconds| Utc::now().timestamp() + seconds);
        let record = GateRecord {
            workflow_run_id: ctx.workflow_run.id,
            node_id: ctx.node.id.clone(),
            kind: params.kind,
            status: "pending".into(),
            label: params.label.clone(),
            condition: params.condition.to_value(),
            metadata: params.metadata.clone(),
        };
        let gate = ctx.db.create_gate(record.to_wire_value()?).await?;
        let gate_id = gate
            .get("id")
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<Uuid>().ok());
        let gate_state = GateState {
            gate_id,
            deadline_unix,
            poll_interval: params.poll_interval_seconds,
        };
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Waiting,
                Some(node_run.attempt + 1),
                None,
                None,
                Some(gate_state.to_wire_value()?),
                Some("gate_pending".into()),
                None,
            )
            .await?;
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        enqueue_gate_poll(ctx, params.poll_interval_seconds).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

/// is the gate currently passable? condition gates auto-evaluate their `when`; manual/external gates
/// read the persisted gate row's status (opened by the ui or an external system via the api).
async fn gate_is_open<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    params: &runinator_workflows::GateParameters,
    gate_id: Option<Uuid>,
) -> Result<bool, SendableError> {
    match params.kind {
        GateKind::Condition => {
            let context = runtime_context(ctx).await;
            runinator_workflows::evaluate_workflow_condition(&params.condition, &context)
                .map_err(|err| -> SendableError { Box::new(err) })
        }
        GateKind::Manual | GateKind::External => {
            let Some(gate_id) = gate_id else {
                return Ok(false);
            };
            let Some(gate) = ctx.db.fetch_gate(gate_id).await? else {
                return Ok(false);
            };
            let status = gate
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(matches!(status, "open" | "passed"))
        }
    }
}

/// apply a terminal status to the gate row (passed/timed_out), stamping resolution fields.
async fn mark_gate<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    gate_id: Uuid,
    status: &str,
    reason: Option<String>,
    resolved_by: Option<String>,
) -> Result<(), SendableError> {
    let Some(mut gate) = ctx.db.fetch_gate(gate_id).await? else {
        return Ok(());
    };
    if let Some(object) = gate.as_object_mut() {
        object.insert("status".into(), status.into());
        object.insert("resolved_at".into(), Utc::now().timestamp().into());
        if let Some(reason) = reason {
            object.insert("reason".into(), reason.into());
        }
        if let Some(resolved_by) = resolved_by {
            object.insert("resolved_by".into(), resolved_by.into());
        }
    }
    ctx.db.update_gate(gate_id, gate).await?;
    Ok(())
}

/// schedule the next gate re-check. the event-driven ready queue does not re-poll parked nodes, so a
/// gate re-arms its own wake like the wait node.
async fn enqueue_gate_poll<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    poll_interval_seconds: i64,
) -> Result<(), SendableError> {
    let poll_at = Utc::now() + chrono::Duration::seconds(poll_interval_seconds);
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "gate_poll",
        runinator_models::json!({ "node_id": ctx.node.id }),
    );
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), poll_at)
        .await?;
    Ok(())
}
