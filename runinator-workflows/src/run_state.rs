// pure predicates over node-run history used by control-flow nodes (join/race/map).
//
// these read sibling node-run history rather than the run `state` blob, so they are backend-neutral
// and shared by both the scheduler node engine and the web-service reducer. typed manipulation of the
// `state` blob itself lives next to the frame types in `runinator-models::workflow_state`.

use runinator_models::workflows::{WorkflowNodeRun, WorkflowStatus};
use uuid::Uuid;

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

/// true when the join's `wait_for` branches satisfy `mode`, counting only runs after `since`.
///
/// `since` is the join's own last settled run, and it is what scopes the answer to one lap. without
/// it a join inside a loop body reads the *previous* lap's branch results as this lap's: the first
/// branch of lap two to arrive finds every other branch still marked `Succeeded` from lap one, so
/// the join fires immediately and every branch walks through it unjoined. `None` means no prior
/// settle — the first visit, where the whole history counts.
pub fn join_satisfied(
    wait_for: &[String],
    mode: BranchPolicy,
    node_runs: &[WorkflowNodeRun],
    since: Option<Uuid>,
) -> bool {
    let succeeded = |node_id: &String| {
        latest_node_run_after(node_runs, node_id, since)
            .is_some_and(|run| run.status == WorkflowStatus::Succeeded)
    };
    match mode {
        BranchPolicy::All => wait_for.iter().all(succeeded),
        BranchPolicy::Any | BranchPolicy::FirstSuccess => wait_for.iter().any(succeeded),
    }
}

/// the latest run for `node_id` recorded after `since`, by highest id.
fn latest_node_run_after<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
    since: Option<Uuid>,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .filter(|run| since.is_none_or(|since| run.id > since))
        .max_by_key(|run| run.id)
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

/// stable name for a branch policy, recorded in join output.
pub fn branch_policy_name(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}
