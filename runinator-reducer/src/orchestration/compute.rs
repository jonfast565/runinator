use super::context::runtime_context;
use super::*;
use runinator_models::workflows::WorkflowDefinition;
use runinator_workflows::{
    ComputeOutcome, FunctionTable, PureIntrinsics, parse_program, run_program_with,
};

const PROGRAM_KEY: &str = "program";

// the registry of (provider, function) entry points the reducer can evaluate in-process. each
// entry must be a function the stored provider metadata marks `pure: true` (the contract) *and*
// one whose interpreter ws can host (the gate). today that is only the std library's `run` entry,
// backed by `runinator_workflows::PureIntrinsics`; adding another pure provider ws can host is a
// one-line extension here plus a matching in-process interpreter.
const INPROCESS_PURE_FNS: &[(&str, &str)] = &[("std", "run")];

/// whether an action node should be evaluated in-process by the reducer. effectful entry points
/// (`std.exec`) and every other provider dispatch to the worker.
pub(super) fn is_inprocess_compute(node: &WorkflowNode) -> bool {
    node.action.as_ref().is_some_and(|action| {
        INPROCESS_PURE_FNS.iter().any(|(provider, function)| {
            action.provider == *provider && action.function == *function
        })
    })
}

/// evaluate a pure `std.run` compute node in the reducer, mirroring the Switch arm: create a node
/// run, run the program against the runtime context, and either transition on `return`/fallthrough
/// or set the active node directly on `goto`.
pub(super) async fn process_compute_node<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
    nodes: &[WorkflowNode],
) -> Result<(), SendableError> {
    let action = node
        .action
        .as_ref()
        .ok_or_else(|| crate::errors::ACTION_CONFIG_MISSING.error(&node.id))?;
    let program_value = action
        .configuration
        .as_value()
        .get(PROGRAM_KEY)
        .cloned()
        .ok_or_else(|| {
            crate::errors::COMPUTE_NODE_FAILED.error(format!("{}: missing program", node.id))
        })?;
    let program =
        parse_program(&program_value).map_err(|err| -> SendableError { Box::new(err) })?;

    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let functions = FunctionTable::from_metadata(workflow.definition.metadata.get("functions"))
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let outcome = run_program_with(&program, &context, &PureIntrinsics, Some(&functions))
        .map_err(|err| -> SendableError { Box::new(err) })?;

    match outcome {
        ComputeOutcome::Return(value) | ComputeOutcome::Fallthrough(value) => {
            transitions::transition_from_node(
                db,
                workflow_run,
                node,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(value),
                Some("compute_evaluated".into()),
                node_runs,
            )
            .await?;
        }
        ComputeOutcome::Goto(target) => {
            let target = resolve_goto_target(&target, nodes);
            db.update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Succeeded,
                Some(node_run.attempt + 1),
                None,
                Some(Value::Null),
                None,
                Some("compute_goto".into()),
                None,
            )
            .await?;
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(target),
                None,
                None,
            )
            .await?;
        }
    }
    Ok(())
}

// resolve a goto target: a real node id is used directly; the synthetic `done`/`fail` map to the
// workflow's end/fail node ids.
fn resolve_goto_target(target: &str, nodes: &[WorkflowNode]) -> String {
    if nodes.iter().any(|node| node.id == target) {
        return target.to_string();
    }
    let kind = match target {
        "done" => Some(WorkflowNodeKind::End),
        "fail" => Some(WorkflowNodeKind::Fail),
        _ => None,
    };
    if let Some(kind) = kind
        && let Some(node) = nodes.iter().find(|node| node.kind == kind)
    {
        return node.id.clone();
    }
    target.to_string()
}
