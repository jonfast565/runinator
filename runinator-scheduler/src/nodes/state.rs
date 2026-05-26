use runinator_models::workflows::{WorkflowNodeRun, WorkflowStatus};
use runinator_workflows::BranchPolicy;
use serde_json::Value;

use crate::context::latest_node_run;

pub(super) struct QueuedState {
    pub(super) target: String,
    pub(super) state: Value,
}

pub(super) fn merge_state(base: &Value, key: &str, value: Value) -> Value {
    let mut object = base.as_object().cloned().unwrap_or_default();
    object.insert(key.into(), value);
    Value::Object(object)
}

pub(super) fn pop_state_queue(
    state: &Value,
    frame_key: &str,
    queue_key: &str,
) -> Option<QueuedState> {
    let mut root = state.as_object().cloned().unwrap_or_default();
    let mut frame = root.get(frame_key)?.as_object()?.clone();
    let mut remaining = frame.get(queue_key)?.as_array()?.clone();
    if remaining.is_empty() {
        return None;
    }
    let target = remaining.remove(0).as_str()?.to_string();
    frame.insert(queue_key.into(), Value::Array(remaining));
    root.insert(frame_key.into(), Value::Object(frame));
    Some(QueuedState {
        target,
        state: Value::Object(root),
    })
}

pub(super) fn join_satisfied(
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

pub(super) fn race_winner(
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

pub(super) fn latest_status(
    node_id: &str,
    node_runs: &[WorkflowNodeRun],
) -> Option<WorkflowStatus> {
    latest_node_run(node_runs, node_id).map(|run| run.status)
}

pub(super) fn append_completed_map_item(
    frame: Option<Value>,
    target: &str,
    node_runs: &[WorkflowNodeRun],
) -> Option<Value> {
    let mut frame = frame?;
    let latest = latest_node_run(node_runs, target)?;
    if latest.status != WorkflowStatus::Succeeded {
        return Some(frame);
    }
    let object = frame.as_object_mut()?;
    let index = object
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let outputs = object
        .entry("outputs")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(outputs) = outputs.as_array_mut() else {
        return Some(Value::Object(object.clone()));
    };
    if outputs.len() as i64 <= index {
        outputs.push(latest.output_json.clone().unwrap_or(Value::Null));
        object.insert("index".into(), Value::from(index + 1));
    }
    Some(frame)
}

pub(super) fn parse_workflow_status(value: &str) -> Option<WorkflowStatus> {
    match value {
        "queued" => Some(WorkflowStatus::Queued),
        "running" => Some(WorkflowStatus::Running),
        "paused" => Some(WorkflowStatus::Paused),
        "waiting" => Some(WorkflowStatus::Waiting),
        "approval_required" => Some(WorkflowStatus::ApprovalRequired),
        "blocked" => Some(WorkflowStatus::Blocked),
        "succeeded" => Some(WorkflowStatus::Succeeded),
        "failed" => Some(WorkflowStatus::Failed),
        "timed_out" => Some(WorkflowStatus::TimedOut),
        "canceled" => Some(WorkflowStatus::Canceled),
        _ => None,
    }
}

pub(super) fn branch_policy_name(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}
