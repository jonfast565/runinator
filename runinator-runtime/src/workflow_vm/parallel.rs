//! parallel behavior for the workflow interpreter.

use super::context::stable_id;
use super::failure::fail;
use super::{InstructionOutcome, WorkflowVmStep, transitions};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowForkFrame, WorkflowFrame,
    WorkflowMapFrame, WorkflowRaceFrame,
};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn fork(
    mut parent: WorkflowContinuation,
    targets: &[usize],
    key: &str,
    kind: Option<&str>,
) -> WorkflowVmStep {
    if targets.is_empty() {
        return fail(parent, "fork needs at least one target".into());
    }
    let mut children = Vec::with_capacity(targets.len());
    for (branch, target) in targets.iter().enumerate() {
        let mut child = parent.clone();
        child.id = stable_id(
            parent.id,
            &format!("{}:{key}:{branch}", kind.unwrap_or("fork")),
        );
        child.parent_id = Some(parent.id);
        child.fork_key = Some(key.to_owned());
        // Entries before the fork belong to the parent branch. Each child will collect only the
        // nodes it enters after its own target, preventing duplicated history at fan-out.
        child.pending_node_entries.clear();
        child.instruction_pointer = *target;
        child.awaiting_effect_id = None;
        child.status = WorkflowContinuationStatus::Runnable;
        child.frames.push(WorkflowFrame::Fork(WorkflowForkFrame {
            fork_key: key.to_owned(),
            parent_id: parent.id,
            branch_index: branch as u64,
        }));
        children.push(child);
    }
    parent.instruction_pointer += 1;
    parent.status = WorkflowContinuationStatus::Joined;
    WorkflowVmStep::Fork {
        parent,
        children,
        join_key: key.to_owned(),
    }
}

pub(super) fn fork_map(
    parent: WorkflowContinuation,
    targets: &[usize],
    map_key: &str,
    items: &[Value],
) -> WorkflowVmStep {
    let WorkflowVmStep::Fork {
        parent,
        mut children,
        join_key,
    } = fork(parent, targets, map_key, Some("map"))
    else {
        unreachable!("non-empty map targets always fork")
    };
    for (index, child) in children.iter_mut().enumerate() {
        child.frames.retain(|frame| {
            !matches!(frame, WorkflowFrame::Map(frame) if frame.map_key == map_key && frame.item_index.is_none())
        });
        child
            .locals
            .insert(format!("{map_key}.item"), items[index].clone());
        child
            .locals
            .insert(format!("{map_key}.index"), Value::from(index as i64));
        child.frames.push(WorkflowFrame::Map(WorkflowMapFrame {
            map_key: map_key.to_owned(),
            body: child.instruction_pointer,
            exit: 0,
            concurrency: 0,
            next_index: 0,
            items: Vec::new(),
            results: Vec::new(),
            item: Some(items[index].clone()),
            item_index: Some(index as u64),
        }));
    }
    WorkflowVmStep::Fork {
        parent,
        children,
        join_key,
    }
}

pub(super) fn fork_instruction(
    continuation: WorkflowContinuation,
    targets: &[usize],
    join_key: &str,
) -> InstructionOutcome {
    Break(fork(continuation, targets, join_key, None))
}

pub(super) fn race(
    mut continuation: WorkflowContinuation,
    targets: &[usize],
    race_key: &str,
    winner: &runinator_models::workflow_vm::WorkflowBranchPolicy,
) -> InstructionOutcome {
    if targets.is_empty() {
        return Break(fail(continuation, "race needs at least one target".into()));
    }
    continuation
        .frames
        .push(WorkflowFrame::Race(WorkflowRaceFrame {
            race_key: race_key.to_owned(),
            expected: targets.len() as u64,
            winner_policy: *winner,
            winner: None,
            winner_value: None,
        }));
    let WorkflowVmStep::Fork {
        parent,
        mut children,
        join_key,
    } = fork(continuation, targets, race_key, Some("race"))
    else {
        unreachable!("non-empty race targets always fork")
    };
    // The race coordinator belongs only to the parked parent. A contender retains
    // its fork provenance, but cannot independently nominate or overwrite a winner.
    for child in &mut children {
        child
            .frames
            .retain(|frame| !matches!(frame, WorkflowFrame::Race(_)));
    }
    Break(WorkflowVmStep::Fork {
        parent,
        children,
        join_key,
    })
}

pub(super) fn begin_map(
    mut continuation: WorkflowContinuation,
    map_key: &str,
    body: &usize,
    exit: &usize,
    concurrency: &u64,
) -> InstructionOutcome {
    if *concurrency == 0 {
        return Break(fail(
            continuation,
            "map concurrency must be greater than zero".into(),
        ));
    }
    let is_map_child = continuation.frames.iter().any(|frame| {
        matches!(frame, WorkflowFrame::Map(frame) if frame.map_key == *map_key && frame.item_index.is_some())
    });
    if is_map_child {
        // A map body returns to its map node through the normal graph edge. The
        // compiler evaluates `items` again on that entry; discard that stable
        // expression value and report the body's result beneath it.
        let _items = continuation.stack.pop();
        let value = continuation.stack.pop().unwrap_or(Value::Null);
        return Break(transitions::join(continuation, map_key.to_owned(), value));
    }
    let Some(Value::Array(items)) = continuation.stack.pop() else {
        return Break(fail(
            continuation,
            format!("map '{map_key}' needs an array value"),
        ));
    };
    if items.is_empty() {
        continuation.stack.push(Value::Array(Vec::new()));
        continuation.instruction_pointer = *exit;
        return Continue(continuation);
    }
    let count = (*concurrency as usize).min(items.len());
    continuation
        .frames
        .push(WorkflowFrame::Map(WorkflowMapFrame {
            map_key: map_key.to_owned(),
            body: *body,
            exit: *exit,
            concurrency: *concurrency,
            next_index: count as u64,
            items: items.clone(),
            results: Vec::new(),
            item: None,
            item_index: None,
        }));
    let targets = std::iter::repeat_n(*body, count).collect::<Vec<_>>();
    Break(fork_map(continuation, &targets, map_key, &items[..count]))
}

pub(super) fn join(
    mut continuation: WorkflowContinuation,
    join_key: &str,
    expected: &u64,
    mode: &runinator_models::workflow_vm::WorkflowBranchPolicy,
) -> InstructionOutcome {
    let value = continuation.stack.pop().unwrap_or(Value::Null);
    continuation.frames.push(WorkflowFrame::Join(
        runinator_models::workflow_vm::WorkflowJoinFrame {
            join_key: join_key.to_owned(),
            expected: *expected,
            mode: *mode,
            arrivals: Vec::new(),
        },
    ));
    Break(transitions::join(continuation, join_key.to_owned(), value))
}
