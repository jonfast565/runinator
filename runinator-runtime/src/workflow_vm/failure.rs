//! failure behavior for the workflow interpreter.

use super::effects::yield_effect;
use super::interrupts::interrupt_frame;
use super::{WorkflowVmStep, step, transitions};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowFailure, WorkflowFailureKind,
    WorkflowFrame, WorkflowInterruptOutcome, WorkflowModule, WorkflowTryFrame, WorkflowTryPhase,
};

pub(super) fn fail(mut continuation: WorkflowContinuation, message: String) -> WorkflowVmStep {
    // a handler cannot fail the run. the strongest thing it can say is `resume fail`, and a handler
    // that breaks on its own gives the interrupted thread back untouched instead.
    if let Some(frame) = interrupt_frame(&continuation) {
        let interrupted_continuation_id = frame.interrupted_continuation_id;
        let instruction_pointer = frame.resume_instruction_pointer;
        continuation.status = WorkflowContinuationStatus::Failed;
        return WorkflowVmStep::InterruptResolved {
            handler: continuation,
            interrupted_continuation_id,
            outcome: WorkflowInterruptOutcome::Resume {
                instruction_pointer,
            },
        };
    }
    continuation.status = WorkflowContinuationStatus::Failed;
    WorkflowVmStep::Failed {
        continuation,
        message,
    }
}

/// Route a failure through the nearest structured try frame, then through the durable
/// compensation stack. This keeps the decision entirely inside the continuation; a host never
/// needs to rediscover graph ancestry from node-run history.
pub(super) fn handle_failure(
    module: &WorkflowModule,
    continuation: WorkflowContinuation,
    message: String,
) -> WorkflowVmStep {
    handle_classified_failure(module, continuation, WorkflowFailure::failed(message))
}

/// The `on_failure` / `on_timeout` / `on_reject` edges of one graph node are compiled into a try
/// frame whose targets differ by classification, so the routing decision needs the kind and not
/// only the message.
pub(super) fn handle_classified_failure(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    failure: WorkflowFailure,
) -> WorkflowVmStep {
    if let Some(frame) = continuation
        .frames
        .iter_mut()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) => Some(frame),
            _ => None,
        })
        && frame.pause_on_failure
        && frame.pending_failure.is_none()
    {
        frame.pending_failure = Some(failure);
        frame.paused = true;
        return transitions::pause(continuation, "debug-failure");
    }
    route_classified_failure(module, continuation, failure)
}

pub(super) fn route_classified_failure(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    failure: WorkflowFailure,
) -> WorkflowVmStep {
    let message = failure.message;
    if let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Try(_)))
    {
        let frame = match continuation.frames.remove(position) {
            WorkflowFrame::Try(frame) => frame,
            _ => unreachable!("try frame position was checked"),
        };
        if frame.phase == WorkflowTryPhase::Body {
            let classified = match failure.kind {
                WorkflowFailureKind::TimedOut => frame.on_timeout,
                WorkflowFailureKind::Rejected => frame.on_reject,
                WorkflowFailureKind::Failed | WorkflowFailureKind::Canceled => None,
            };
            if let Some(catch) = classified.or(frame.catch) {
                continuation
                    .frames
                    .push(WorkflowFrame::Try(WorkflowTryFrame {
                        phase: WorkflowTryPhase::Catch,
                        ..frame
                    }));
                continuation.instruction_pointer = catch;
                return step(module, continuation);
            }
        }
        if let Some(finally) = frame.finally {
            continuation
                .frames
                .push(WorkflowFrame::Try(WorkflowTryFrame {
                    phase: WorkflowTryPhase::Finally,
                    pending_failure: Some(message),
                    ..frame
                }));
            continuation.instruction_pointer = finally;
            return step(module, continuation);
        }
    }

    if let Some(position) = continuation
        .frames
        .iter()
        .position(|frame| matches!(frame, WorkflowFrame::Compensation(_)))
    {
        continuation.locals.insert(
            "__workflow_vm_compensation_failure".into(),
            Value::String(message.clone()),
        );
        let frame = match &mut continuation.frames[position] {
            WorkflowFrame::Compensation(frame) => frame,
            _ => unreachable!("compensation frame position was checked"),
        };
        if let Some(request) = frame.pending.pop() {
            frame.active = Some(request.clone());
            return yield_effect(continuation, request);
        }
    }
    fail(continuation, message)
}
