use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;

/// evaluate the assertions in an assert node's parameters against the runtime context. returns
/// the list of violations (empty → all passed). each entry in `parameters.assertions` must be
/// `{ "name": string, "condition": condition_object, "message"?: string }`.
pub(super) fn evaluate_assertions(params: &Value, context: &Value) -> Vec<AssertViolation> {
    let assertions = params
        .get("assertions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut violations = Vec::new();
    for assertion in &assertions {
        let name = assertion
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        let condition = assertion.get("condition").cloned().unwrap_or(Value::Null);
        let passed = runinator_workflows::evaluate_condition(&condition, context).unwrap_or(false);
        if !passed {
            let message = assertion
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Assertion failed")
                .to_string();
            violations.push(AssertViolation { name, message });
        }
    }
    violations
}

/// process an assert node: evaluates all named assertions inline; fails with a structured
/// violation list if any assertion does not hold.
pub(super) struct AssertOp;

impl AssertOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
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
        let context = runtime_context(ctx).await;
        let params: Value = ctx.node.parameters.clone().into();
        let violations = evaluate_assertions(&params, &context);
        let passed = violations.is_empty();
        let output = AssertOutput { passed, violations };
        let (status, reason) = if passed {
            (WorkflowStatus::Succeeded, "assert_passed")
        } else {
            (WorkflowStatus::Failed, "assert_failed")
        };
        transition_from_node(
            ctx,
            &node_run,
            status,
            Some(output.to_wire_value()?),
            Some(reason.into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
