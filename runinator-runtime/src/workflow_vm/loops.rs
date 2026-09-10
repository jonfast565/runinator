//! loops behavior for the workflow interpreter.

use super::InstructionOutcome;
use super::failure::handle_failure;
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowFrame, WorkflowLoopFrame, WorkflowModule,
};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn next_loop(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    loop_key: &str,
) -> InstructionOutcome {
    let Some(position) = continuation.frames.iter().rposition(
        |frame| matches!(frame, WorkflowFrame::Loop(frame) if frame.loop_key == *loop_key),
    ) else {
        return Break(handle_failure(
            module,
            continuation,
            format!("next_loop '{loop_key}' has no frame"),
        ));
    };
    let mut frame = match continuation.frames.remove(position) {
        WorkflowFrame::Loop(frame) => frame,
        _ => unreachable!(),
    };
    if let Some(value) = continuation.stack.pop() {
        frame.results.push(value);
    }
    frame.index += 1;
    advance_loop(&mut continuation, frame);
    Continue(continuation)
}

pub(super) fn begin_loop(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    loop_key: &str,
    body: &usize,
    exit: &usize,
    max_iterations: &Option<u64>,
) -> InstructionOutcome {
    let existing = continuation.frames.iter().position(
        |frame| matches!(frame, WorkflowFrame::Loop(frame) if frame.loop_key == *loop_key),
    );
    let frame = if let Some(position) = existing {
        // The compiler evaluates `items` on every graph re-entry. Discard that
        // stable expression result, then retain the body result beneath it.
        let _items = continuation.stack.pop();
        let mut frame = match continuation.frames.remove(position) {
            WorkflowFrame::Loop(frame) => frame,
            _ => unreachable!("loop frame position was checked"),
        };
        if let Some(result) = continuation.stack.pop() {
            frame.results.push(result);
        }
        frame.index += 1;
        frame
    } else {
        let Some(Value::Array(items)) = continuation.stack.pop() else {
            return Break(handle_failure(
                module,
                continuation,
                format!("loop '{loop_key}' needs an array value"),
            ));
        };
        WorkflowLoopFrame {
            loop_key: loop_key.to_owned(),
            body: *body,
            exit: *exit,
            index: 0,
            items,
            results: Vec::new(),
            max_iterations: *max_iterations,
        }
    };
    advance_loop(&mut continuation, frame);
    Continue(continuation)
}

pub(super) fn reenter(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    reentry_key: &str,
    target: &usize,
    exhausted: &Option<usize>,
    max_visits: &u64,
) -> InstructionOutcome {
    let position = continuation.frames.iter().position(
        |frame| matches!(frame, WorkflowFrame::Reentry(frame) if frame.reentry_key == *reentry_key),
    );
    let visits = match position {
        Some(position) => match &mut continuation.frames[position] {
            WorkflowFrame::Reentry(frame) => {
                frame.visits += 1;
                frame.visits
            }
            _ => unreachable!("reentry frame position was checked"),
        },
        None => {
            continuation.frames.push(WorkflowFrame::Reentry(
                runinator_models::workflow_vm::WorkflowReentryFrame {
                    reentry_key: reentry_key.to_owned(),
                    visits: 1,
                    max_visits: *max_visits,
                },
            ));
            1
        }
    };
    if visits > *max_visits {
        match exhausted {
            Some(target) => continuation.instruction_pointer = *target,
            None => {
                return Break(handle_failure(
                    module,
                    continuation,
                    format!("reentry '{reentry_key}' exhausted after {max_visits} visits"),
                ));
            }
        }
    } else {
        continuation.instruction_pointer = *target;
    }
    Continue(continuation)
}

// both entry forms apply the same iteration limit, bindings, and completion result.
fn advance_loop(continuation: &mut WorkflowContinuation, frame: WorkflowLoopFrame) {
    let has_next = frame.index < frame.items.len() as u64
        && frame
            .max_iterations
            .map(|limit| frame.index < limit)
            .unwrap_or(true);
    if has_next {
        continuation.locals.insert(
            format!("{}.item", frame.loop_key),
            frame.items[frame.index as usize].clone(),
        );
        continuation.locals.insert(
            format!("{}.index", frame.loop_key),
            Value::from(frame.index as i64),
        );
        continuation.instruction_pointer = frame.body;
        continuation.frames.push(WorkflowFrame::Loop(frame));
    } else {
        continuation.stack.push(Value::Array(frame.results));
        continuation.instruction_pointer = frame.exit;
    }
}
