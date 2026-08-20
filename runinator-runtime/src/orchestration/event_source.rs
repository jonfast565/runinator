use super::context::is_reentry_stale;
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;

struct EventSourceParams {
    event_type: String,
    max_events: Option<i64>,
    filter: Value,
}

fn parse_event_source_params(node: &WorkflowNode) -> EventSourceParams {
    let params: Value = node.parameters.clone().into();
    EventSourceParams {
        event_type: params
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or("*")
            .to_string(),
        max_events: params.get("max").and_then(Value::as_i64),
        filter: params.get("filter").cloned().unwrap_or(Value::Null),
    }
}

/// true when an event's `type` field matches the subscription type (`"*"` is a wildcard).
pub(super) fn event_type_matches(event: &Value, expected_type: &str) -> bool {
    if expected_type == "*" {
        return true;
    }
    event
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t == expected_type)
}

/// true when the event passes the optional filter condition evaluated against `context`.
fn event_passes_filter(event: &Value, filter: &Value, context: &Value) -> bool {
    if matches!(filter, Value::Null) {
        return true;
    }
    // merge the event into the context so the filter can reference `event.*`.
    let mut ctx = context.clone();
    if let Some(obj) = ctx.as_object_mut() {
        obj.insert("event".into(), event.clone());
    }
    runinator_workflows::evaluate_condition(filter, &ctx).unwrap_or(false)
}

async fn park_event_source<T: RuntimeStore>(
    ctx: &super::execution::NodeExecutionContext<'_, T>,
    state: EventSourceState,
    is_first_visit: bool,
    node_run_id: Uuid,
    attempt: i64,
) -> Result<(), SendableError> {
    ctx.db
        .update_workflow_node_run(
            node_run_id,
            WorkflowStatus::Waiting,
            Some(attempt + 1),
            None,
            None,
            Some(state.to_wire_value()?),
            Some(
                if is_first_visit {
                    "event_source_started"
                } else {
                    "event_source_re_parked"
                }
                .into(),
            ),
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
    Ok(())
}

/// process an event_source node: parks waiting for matching events. an external event-delivery
/// endpoint drives the run on each matching event; the node re-parks after each iteration until
/// `max_events` is reached or the timeout elapses.
///
/// event delivery: `POST /workflow_runs/{id}/events/{node_id}` with the event payload. the ws
/// layer drives the run with the event in state, the body executes one iteration, then the node
/// re-parks.
pub(super) struct EventSourceOp;

impl EventSourceOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let params = parse_event_source_params(ctx.node);
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
            if timed_out_since_created(ctx.timing(), node_run) {
                time_out(ctx, node_run, "EventSource timed out").await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            // check if the current drive carries an inbound event payload (stored in run state).
            let state = node_run.state.decode::<EventSourceState>().ok();
            let events_processed = state.as_ref().map(|s| s.events_processed).unwrap_or(0);
            let deadline = state.as_ref().and_then(|s| s.deadline_unix);
            if let Some(deadline) = deadline
                && Utc::now().timestamp() >= deadline
            {
                transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(runinator_models::json!({ "events_processed": events_processed })),
                    Some("event_source_done".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            // check if an inbound event was delivered (ws stamps it under state.pending_event).
            let pending_event = ctx
                .workflow_run
                .state
                .pointer(&format!("/event_source_{}/pending_event", ctx.node.id))
                .cloned();
            if let Some(event) = pending_event {
                let context = runinator_workflows::outputs_context(
                    &ctx.workflow_run.parameters,
                    &std::collections::HashMap::new(),
                );
                if event_type_matches(&event, &params.event_type)
                    && event_passes_filter(&event, &params.filter, &context)
                {
                    let new_count = events_processed + 1;
                    if params.max_events.is_some_and(|max| new_count >= max) {
                        // max reached; succeed and stop.
                        transition_from_node(
                            ctx,
                            node_run,
                            WorkflowStatus::Succeeded,
                            Some(runinator_models::json!({ "events_processed": new_count })),
                            Some("event_source_max_reached".into()),
                        )
                        .await?;
                        return Ok(ReadyNodeDisposition::Complete);
                    }
                    // re-park with incremented counter; body runs on next drive from ws.
                    let new_state = EventSourceState {
                        event_type: params.event_type.clone(),
                        events_processed: new_count,
                        deadline_unix: deadline,
                        max_events: params.max_events,
                    };
                    park_event_source(ctx, new_state, false, node_run.id, node_run.attempt).await?;
                    return Ok(ReadyNodeDisposition::Complete);
                }
            }
            // no matching event yet; keep waiting.
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // first visit: park and subscribe.
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
        let deadline_unix = ctx.node.timeout_seconds.map(|t| Utc::now().timestamp() + t);
        let state = EventSourceState {
            event_type: params.event_type.clone(),
            events_processed: 0,
            deadline_unix,
            max_events: params.max_events,
        };
        park_event_source(ctx, state, true, node_run.id, node_run.attempt).await?;
        arm_node_timeout(ctx).await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
