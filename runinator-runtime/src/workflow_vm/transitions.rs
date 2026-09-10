//! transitions behavior for the workflow interpreter.

use super::failure::handle_failure;
use super::interrupts::{interrupt_frame, resolve_interrupt};
use super::{InstructionOutcome, WorkflowVmStep};
use runinator_models::interrupt::InterruptMode;
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowModule,
};
use std::ops::ControlFlow::Break;

pub(super) fn return_value(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
) -> InstructionOutcome {
    // a handler region that runs off its end without an explicit `resume` still has to
    // hand control back, or the thread it froze would never move again.
    if interrupt_frame(&continuation).is_some() {
        return Break(resolve_interrupt(
            module,
            continuation,
            InterruptMode::Resume,
        ));
    }
    let value = continuation.stack.pop().unwrap_or(Value::Null);
    continuation.status = WorkflowContinuationStatus::Succeeded;
    Break(WorkflowVmStep::Complete {
        continuation,
        value,
    })
}

pub(super) fn fail_instruction(
    continuation: WorkflowContinuation,
    module: &WorkflowModule,
    message: &str,
) -> InstructionOutcome {
    Break(handle_failure(module, continuation, message.to_owned()))
}

/// Park a branch after its instruction has prepared any join frame.
pub(super) fn join(
    mut continuation: WorkflowContinuation,
    join_key: String,
    value: Value,
) -> WorkflowVmStep {
    continuation.status = WorkflowContinuationStatus::Joined;
    WorkflowVmStep::Joined {
        continuation,
        join_key,
        value,
    }
}

/// Preserve an operator pause across subsequent effect settlement.
pub(super) fn pause(mut continuation: WorkflowContinuation, join_key: &str) -> WorkflowVmStep {
    continuation.status = WorkflowContinuationStatus::Paused;
    continuation.operator_paused = true;
    WorkflowVmStep::Joined {
        continuation,
        join_key: join_key.to_owned(),
        value: Value::Null,
    }
}
