//! interrupts behavior for the workflow interpreter.

use super::context::stable_id;
use super::failure::handle_failure;
use super::{InstructionOutcome, WorkflowVmStep};
use runinator_models::interrupt::{InterruptMode, InterruptSource};
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest, WorkflowFailure,
    WorkflowFailureKind, WorkflowFrame, WorkflowInterruptFrame, WorkflowInterruptOutcome,
    WorkflowModule,
};
use std::ops::ControlFlow::{Break, Continue};

/// The interrupt source a settled effect represents, in [`InterruptSource::ALL`] precedence.
///
/// `Retry` is the one source with no counterpart here: a re-dispatch is the effect host's business
/// and never reaches the VM as a step, so nothing in a continuation can observe it.
pub(super) fn arrival_interrupt_source(
    request: Option<&WorkflowEffectRequest>,
    result: &Result<Value, WorkflowFailure>,
) -> Option<InterruptSource> {
    if let Err(failure) = result {
        return match failure.kind {
            WorkflowFailureKind::TimedOut => Some(InterruptSource::Timeout),
            WorkflowFailureKind::Failed => Some(InterruptSource::Failure),
            // a cancel is not a condition a handler gets to reconsider.
            WorkflowFailureKind::Canceled | WorkflowFailureKind::Rejected => None,
        };
    }
    match request? {
        WorkflowEffectRequest::Timer { .. } | WorkflowEffectRequest::TimerDelay { .. } => {
            Some(InterruptSource::Wake)
        }
        WorkflowEffectRequest::ChildRun { .. } => Some(InterruptSource::Child),
        WorkflowEffectRequest::Signal { .. }
        | WorkflowEffectRequest::Approval { .. }
        | WorkflowEffectRequest::Input { .. } => Some(InterruptSource::Resolved),
        _ => None,
    }
}

/// Raise a VM-detected interrupt, if everything the fail-open rules ask for holds.
pub(super) fn try_raise_detected(
    module: &WorkflowModule,
    continuation: &WorkflowContinuation,
    source: InterruptSource,
) -> Option<WorkflowVmStep> {
    if interrupt_frame(continuation).is_some() {
        return None;
    }
    let handler = module.interrupt_handler(source)?;
    let location = module.graph_location(continuation.instruction_pointer)?;
    if !location.interruptible {
        return None;
    }
    Some(raise_interrupt(
        module,
        continuation.clone(),
        source,
        Value::Null,
        handler.target,
        "",
    ))
}

pub(super) fn handler_for_pending<'a>(
    handlers: &'a [runinator_models::workflow_vm::WorkflowVmInterruptHandler],
    pending: &runinator_models::workflow_vm::WorkflowPendingInterrupt,
) -> Option<&'a runinator_models::workflow_vm::WorkflowVmInterruptHandler> {
    if pending.source != InterruptSource::Timer {
        return handlers
            .iter()
            .find(|handler| handler.source == pending.source);
    }
    let timer_id = pending.payload.get("timer_id").and_then(Value::as_str)?;
    handlers.iter().find(|handler| {
        handler.source == InterruptSource::Timer && handler.timer_id.as_deref() == Some(timer_id)
    })
}

/// Each periodic occurrence gets its own deterministic handler id. A redelivered wake recreates
/// the same id; the next interval's different due instant creates a new handler.
pub(super) fn timer_discriminator(
    pending: &runinator_models::workflow_vm::WorkflowPendingInterrupt,
) -> String {
    if pending.source != InterruptSource::Timer {
        return String::new();
    }
    let timer_id = pending
        .payload
        .get("timer_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let due_at = pending
        .payload
        .get("due_at")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    format!("timer:{timer_id}:{due_at}")
}

/// Freeze `continuation` and start a handler continuation beside it.
///
/// The handler is a separate continuation rather than a frame on this one, because the interrupted
/// thread must stay exactly where it was: everything the handler does is invisible to it, and the
/// only channel back is the decision its `resume` carries.
/// Build the handler continuation for `source` beside `continuation`, deciding nothing about the
/// interrupted thread itself.
///
/// [`raise_interrupt`] pairs this with a suspend. The engine's retry path calls it directly instead:
/// a thread parked on an effect is already stopped, and suspending it would stop the retried
/// effect's own settlement from resuming it.
pub fn interrupt_handler_continuation(
    module: &WorkflowModule,
    continuation: &WorkflowContinuation,
    source: InterruptSource,
    payload: Value,
    target: usize,
    discriminator: &str,
) -> WorkflowContinuation {
    let location = module.graph_location(continuation.instruction_pointer);
    let frame = WorkflowInterruptFrame {
        source,
        interrupted_continuation_id: continuation.id,
        // the interrupted thread resumes *after* the point it was frozen at, so servicing an
        // interrupt at a safe point cannot re-raise the same one on the next drive.
        resume_instruction_pointer: continuation.instruction_pointer + 1,
        node_start_instruction_pointer: location
            .map(|entry| entry.instruction_start)
            .unwrap_or(continuation.instruction_pointer),
        node_exit_instruction_pointer: location.and_then(|entry| entry.exit_instruction_pointer),
        payload: payload.clone(),
        handled_at_instruction_pointers: vec![continuation.instruction_pointer],
    };

    let mut handler = WorkflowContinuation::start(continuation.workflow_run_id, module.version);
    // the handler id is derived from the interrupted thread, the source, and the caller's
    // discriminator, so a redelivered drive re-raising the same interrupt inserts nothing while a
    // genuinely new occurrence (the next retry attempt) gets its own handler.
    handler.id = stable_id(
        continuation.id,
        &format!("interrupt:{}:{target}:{discriminator}", source.as_str()),
    );
    handler.instruction_pointer = target;
    // the region reads the run's context plus what raised it; it writes nothing back except its
    // `resume` decision.
    handler.locals = continuation.locals.clone();
    handler.locals.insert(
        "interrupt".into(),
        runinator_models::json!({ "source": source.as_str(), "payload": payload }),
    );
    handler.parent_id = Some(continuation.id);
    handler.frames = vec![WorkflowFrame::Interrupt(frame)];
    handler
}

pub(super) fn raise_interrupt(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    source: InterruptSource,
    payload: Value,
    target: usize,
    discriminator: &str,
) -> WorkflowVmStep {
    let handler = interrupt_handler_continuation(
        module,
        &continuation,
        source,
        payload,
        target,
        discriminator,
    );
    continuation.status = WorkflowContinuationStatus::Suspended;
    WorkflowVmStep::Interrupted {
        suspended: continuation,
        handler: Box::new(handler),
        source,
    }
}

/// Finish a handler and say what the thread it suspended should do next.
pub(super) fn resolve_interrupt(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    mode: InterruptMode,
) -> WorkflowVmStep {
    let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Interrupt(_)))
    else {
        return handle_failure(
            module,
            continuation,
            "resume_interrupt has no interrupt frame".into(),
        );
    };
    let frame = match continuation.frames.remove(position) {
        WorkflowFrame::Interrupt(frame) => frame,
        _ => unreachable!("interrupt frame position was checked"),
    };
    let outcome = match mode {
        InterruptMode::Resume => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame.resume_instruction_pointer,
        },
        InterruptMode::Restart => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame.node_start_instruction_pointer,
        },
        // a node with no single exit cannot be stepped past, so `continue` degrades to `resume`
        // rather than guessing a location.
        InterruptMode::Continue => WorkflowInterruptOutcome::Resume {
            instruction_pointer: frame
                .node_exit_instruction_pointer
                .unwrap_or(frame.resume_instruction_pointer),
        },
        InterruptMode::Fail => WorkflowInterruptOutcome::Fail {
            message: format!(
                "interrupt handler for '{}' selected fail",
                frame.source.as_str()
            ),
        },
    };
    continuation.status = WorkflowContinuationStatus::Succeeded;
    WorkflowVmStep::InterruptResolved {
        handler: continuation,
        interrupted_continuation_id: frame.interrupted_continuation_id,
        outcome,
    }
}

/// The interrupt frame a handler continuation carries, if this is one.
pub(super) fn interrupt_frame(
    continuation: &WorkflowContinuation,
) -> Option<&WorkflowInterruptFrame> {
    continuation
        .frames
        .iter()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Interrupt(frame) => Some(frame),
            _ => None,
        })
}

pub(super) fn check_interrupt(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    handlers: &[runinator_models::workflow_vm::WorkflowVmInterruptHandler],
) -> InstructionOutcome {
    let Some(pending) = continuation.pending_interrupt.take() else {
        continuation.instruction_pointer += 1;
        return Continue(continuation);
    };
    match handler_for_pending(handlers, &pending) {
        // fail-open, and the request is consumed either way: a source nobody declared
        // a handler for must not linger and fire at some arbitrary later point.
        None => continuation.instruction_pointer += 1,
        Some(handler) => {
            let discriminator = timer_discriminator(&pending);
            return Break(raise_interrupt(
                module,
                continuation,
                pending.source,
                pending.payload,
                handler.target,
                &discriminator,
            ));
        }
    }
    Continue(continuation)
}

pub(super) fn resume_interrupt(
    continuation: WorkflowContinuation,
    module: &WorkflowModule,
    mode: &InterruptMode,
) -> InstructionOutcome {
    Break(resolve_interrupt(module, continuation, *mode))
}
