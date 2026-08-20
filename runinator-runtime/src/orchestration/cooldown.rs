use super::transitions::transition_from_node;
use super::*;

struct CooldownParams {
    name: String,
    window_seconds: i64,
}

fn parse_cooldown_params(node: &WorkflowNode) -> CooldownParams {
    let params: Value = node.parameters.clone().into();
    CooldownParams {
        name: params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&node.id)
            .to_string(),
        window_seconds: params
            .get("window_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(60),
    }
}

/// seconds left in a cooldown window that was last stamped at `last_run_at`; 0 once elapsed.
#[cfg(test)]
pub(super) fn remaining_seconds(last_run_at: i64, window_seconds: i64, now_unix: i64) -> i64 {
    (last_run_at + window_seconds - now_unix).max(0)
}

/// process a cooldown node: a named cross-run gate. if another run holds the window, this thread of
/// control finishes without entering the body (a clean no-op). otherwise the window is claimed and
/// the node proceeds via `on_success` into the body.
///
/// the claim is one atomic store operation. reading the window, deciding, and then stamping it —
/// which is what this did — let two runs hitting the same gate concurrently both see it elapsed and
/// both enter the body, which is the one thing a gate must not allow.
pub(super) struct CooldownOp;

impl CooldownOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let params = parse_cooldown_params(ctx.node);
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
        let now = Utc::now().timestamp();
        let held = ctx
            .db
            .claim_cooldown(params.name.clone(), params.window_seconds, now)
            .await?;

        // somebody holds the window: skip the body without entering it.
        if let Some(remaining) = held {
            let output = CooldownOutput {
                name: params.name.clone(),
                skipped: true,
                remaining_seconds: remaining,
            };
            ctx.db
                .update_workflow_node_run(
                    node_run.id,
                    WorkflowStatus::Succeeded,
                    Some(node_run.attempt + 1),
                    None,
                    Some(output.to_wire_value()?),
                    None,
                    Some("cooldown_skipped".into()),
                    None,
                )
                .await?;
            // settle through the cursor: the run only succeeds once its last thread of control
            // retires. writing `Succeeded` directly would let one branch inside its cooldown window
            // end the whole run while its siblings were still executing.
            run_state::advance_cursor(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                WorkflowStatus::Succeeded,
                run_state::CursorMove::Retire,
                None,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        // the window is ours, already stamped by the claim: proceed into the body.
        let output = CooldownOutput {
            name: params.name.clone(),
            skipped: false,
            remaining_seconds: 0,
        };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("cooldown_passed".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
