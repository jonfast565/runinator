//! stack behavior for the workflow interpreter.

use super::InstructionOutcome;
use super::context::truthy;
use super::failure::fail;
use runinator_models::value::Value;
use runinator_models::workflow_vm::WorkflowContinuation;
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn enter_node(
    mut continuation: WorkflowContinuation,
    node_id: &str,
) -> InstructionOutcome {
    continuation.pending_node_entries.push(node_id.to_owned());
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn push_const(
    mut continuation: WorkflowContinuation,
    value: &Value,
) -> InstructionOutcome {
    continuation.stack.push(value.clone());
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn load_local(mut continuation: WorkflowContinuation, name: &str) -> InstructionOutcome {
    continuation.stack.push(
        continuation
            .locals
            .get(name)
            .cloned()
            .unwrap_or(Value::Null),
    );
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn store_local(
    mut continuation: WorkflowContinuation,
    name: &str,
) -> InstructionOutcome {
    let Some(value) = continuation.stack.pop() else {
        return Break(fail(
            continuation,
            format!("store_local '{name}' needs a stack value"),
        ));
    };
    continuation.locals.insert(name.to_owned(), value);
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn pop(mut continuation: WorkflowContinuation) -> InstructionOutcome {
    if continuation.stack.pop().is_none() {
        return Break(fail(continuation, "pop needs a stack value".into()));
    }
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn jump(mut continuation: WorkflowContinuation, target: &usize) -> InstructionOutcome {
    continuation.instruction_pointer = *target;
    Continue(continuation)
}

pub(super) fn jump_if_false(
    mut continuation: WorkflowContinuation,
    target: &usize,
) -> InstructionOutcome {
    let Some(value) = continuation.stack.pop() else {
        return Break(fail(
            continuation,
            "jump_if_false needs a stack value".into(),
        ));
    };
    continuation.instruction_pointer = if truthy(&value) {
        continuation.instruction_pointer + 1
    } else {
        *target
    };
    Continue(continuation)
}
