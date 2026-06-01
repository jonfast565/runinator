// pure predicates over node-run history used by control-flow nodes (join/race/map).
//
// these read sibling node-run history rather than the run `state` blob, so they are backend-neutral
// and shared by both the scheduler node engine and the web-service reducer. typed manipulation of the
// `state` blob itself lives next to the frame types in `runinator-models::workflow_state`.

use runinator_models::value::Value;
use runinator_models::workflow_state::MapFrame;
use runinator_models::workflows::{WorkflowNodeRun, WorkflowStatus};

use crate::types::BranchPolicy;

/// the latest run for `node_id`, by highest id.
pub fn latest_node_run<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .max_by_key(|run| run.id)
}

/// the latest status recorded for `node_id`.
pub fn latest_status(node_id: &str, node_runs: &[WorkflowNodeRun]) -> Option<WorkflowStatus> {
    latest_node_run(node_runs, node_id).map(|run| run.status)
}

/// true when the join's `wait_for` branches satisfy `mode`.
pub fn join_satisfied(
    wait_for: &[String],
    mode: BranchPolicy,
    node_runs: &[WorkflowNodeRun],
) -> bool {
    match mode {
        BranchPolicy::All => wait_for
            .iter()
            .all(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded)),
        BranchPolicy::Any | BranchPolicy::FirstSuccess => wait_for
            .iter()
            .any(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded)),
    }
}

/// resolve the winning branch for a race, per `winner` policy.
pub fn race_winner(
    branches: &[String],
    winner: BranchPolicy,
    node_runs: &[WorkflowNodeRun],
) -> Option<String> {
    match winner {
        BranchPolicy::All => {
            if branches
                .iter()
                .all(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded))
            {
                branches.last().cloned()
            } else {
                None
            }
        }
        BranchPolicy::Any | BranchPolicy::FirstSuccess => branches
            .iter()
            .find(|node_id| latest_status(node_id, node_runs) == Some(WorkflowStatus::Succeeded))
            .cloned(),
    }
}

/// append the target's latest succeeded output into the map frame's `outputs` and advance `index`.
pub fn append_completed_map_item(
    mut frame: MapFrame,
    target: &str,
    node_runs: &[WorkflowNodeRun],
) -> MapFrame {
    let Some(latest) = latest_node_run(node_runs, target) else {
        return frame;
    };
    if latest.status != WorkflowStatus::Succeeded {
        return frame;
    }
    if (frame.outputs.len() as i64) <= frame.index {
        frame
            .outputs
            .push(latest.output_json.clone().unwrap_or(Value::Null));
        frame.index += 1;
    }
    frame
}

/// stable name for a branch policy, recorded in join output.
pub fn branch_policy_name(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}
