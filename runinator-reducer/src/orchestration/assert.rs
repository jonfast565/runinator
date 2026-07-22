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
pub(super) async fn process_assert_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let params: Value = node.parameters.clone().into();
    let violations = evaluate_assertions(&params, &context);
    let passed = violations.is_empty();
    let output = AssertOutput { passed, violations };
    let (status, reason) = if passed {
        (WorkflowStatus::Succeeded, "assert_passed")
    } else {
        (WorkflowStatus::Failed, "assert_failed")
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        status,
        Some(output.to_wire_value()?),
        Some(reason.into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) struct AssertHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for AssertHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_assert_node(ctx.db, ctx.workflow_run, ctx.node, ctx.node_runs).await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
