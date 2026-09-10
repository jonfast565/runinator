//! structured behavior for the workflow interpreter.

use super::InstructionOutcome;
use super::effects::yield_effect;
use super::failure::{fail, handle_failure};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowCompensationFrame, WorkflowContinuation, WorkflowEffectRequest, WorkflowFrame,
    WorkflowModule, WorkflowTryFrame, WorkflowTryPhase,
};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn begin_try(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    try_key: &str,
    catch: &Option<usize>,
    on_timeout: &Option<usize>,
    on_reject: &Option<usize>,
    finally: &Option<usize>,
) -> InstructionOutcome {
    let position = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Try(frame) if frame.try_key == *try_key));
    if let Some(position) = position {
        let frame = match continuation.frames.remove(position) {
            WorkflowFrame::Try(frame) => frame,
            _ => unreachable!("try frame position was checked"),
        };
        match frame.phase {
            WorkflowTryPhase::Body | WorkflowTryPhase::Catch if frame.finally.is_some() => {
                let finally = frame.finally.expect("checked");
                continuation
                    .frames
                    .push(WorkflowFrame::Try(WorkflowTryFrame {
                        phase: WorkflowTryPhase::Finally,
                        ..frame
                    }));
                continuation.instruction_pointer = finally;
            }
            WorkflowTryPhase::Finally if frame.pending_failure.is_some() => {
                return Break(handle_failure(
                    module,
                    continuation,
                    frame.pending_failure.expect("checked"),
                ));
            }
            _ => continuation.instruction_pointer += 2,
        }
    } else {
        continuation
            .frames
            .push(WorkflowFrame::Try(WorkflowTryFrame {
                try_key: try_key.to_owned(),
                phase: WorkflowTryPhase::Body,
                catch: *catch,
                on_timeout: *on_timeout,
                on_reject: *on_reject,
                finally: *finally,
                pending_failure: None,
            }));
        continuation.instruction_pointer += 1;
    }
    Continue(continuation)
}

pub(super) fn end_try(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    try_key: &str,
) -> InstructionOutcome {
    let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Try(frame) if frame.try_key == *try_key))
    else {
        return Break(handle_failure(
            module,
            continuation,
            format!("end_try '{try_key}' has no frame"),
        ));
    };
    let frame = match continuation.frames.remove(position) {
        WorkflowFrame::Try(frame) => frame,
        _ => unreachable!(),
    };
    if let Some(finally) = frame.finally {
        continuation
            .frames
            .push(WorkflowFrame::Try(WorkflowTryFrame {
                phase: WorkflowTryPhase::Finally,
                ..frame
            }));
        continuation.instruction_pointer = finally;
    } else if let Some(message) = frame.pending_failure {
        return Break(handle_failure(module, continuation, message));
    } else {
        continuation.instruction_pointer += 1;
    }
    Continue(continuation)
}

pub(super) fn register_compensation(
    mut continuation: WorkflowContinuation,
    request: &WorkflowEffectRequest,
) -> InstructionOutcome {
    let position = continuation
        .frames
        .iter()
        .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)));
    if let Some(position) = position {
        if let WorkflowFrame::Compensation(frame) = &mut continuation.frames[position] {
            frame.pending.push(request.clone());
        }
    } else {
        continuation
            .frames
            .push(WorkflowFrame::Compensation(Box::new(
                WorkflowCompensationFrame {
                    pending: vec![request.clone()],
                    active: None,
                    resume: None,
                },
            )));
    }
    continuation.instruction_pointer += 1;
    Continue(continuation)
}

pub(super) fn begin_compensation(
    mut continuation: WorkflowContinuation,
    resume: &Option<usize>,
) -> InstructionOutcome {
    let Some(position) = continuation
        .frames
        .iter()
        .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)))
    else {
        continuation.instruction_pointer = resume.unwrap_or(continuation.instruction_pointer + 1);
        return Continue(continuation);
    };
    let frame = match &mut continuation.frames[position] {
        WorkflowFrame::Compensation(frame) => frame,
        _ => unreachable!(),
    };
    frame.active = None;
    frame.resume = *resume;
    if let Some(request) = frame.pending.pop() {
        frame.active = Some(request.clone());
        return Break(yield_effect(continuation, request));
    }
    let resume = frame.resume.unwrap_or(continuation.instruction_pointer + 1);
    continuation.frames.remove(position);
    let message = continuation
        .locals
        .remove("__workflow_vm_compensation_failure");
    if let Some(Value::String(message)) = message {
        return Break(fail(continuation, message));
    }
    continuation.instruction_pointer = resume;
    Continue(continuation)
}
