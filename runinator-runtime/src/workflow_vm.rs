//! Host-free interpreter for [`runinator_models::workflow_vm::WorkflowModule`].
//!
//! The machine stops at durable boundaries. Its caller is responsible for assigning effect ids and
//! atomically persisting the returned continuation and effect receipt.

use debug::{sync_debug_config, take_pending_debug_failure};
use effects::yield_effect;
use failure::{fail, handle_classified_failure, route_classified_failure};
use interrupts::{arrival_interrupt_source, try_raise_detected};
use runinator_models::value::Value;
use runinator_models::workflow_state::DebugConfig;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest, WorkflowFailure,
    WorkflowFrame, WorkflowInstruction, WorkflowInterruptOutcome, WorkflowModule,
};

const MAX_INLINE_INSTRUCTIONS: usize = 1_024;

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowVmStep {
    Yield {
        continuation: WorkflowContinuation,
        effect_id: uuid::Uuid,
        sequence: u64,
        request: Box<WorkflowEffectRequest>,
    },
    Fork {
        parent: WorkflowContinuation,
        children: Vec<WorkflowContinuation>,
        join_key: String,
    },
    Joined {
        continuation: WorkflowContinuation,
        join_key: String,
        value: Value,
    },
    Complete {
        continuation: WorkflowContinuation,
        value: Value,
    },
    Failed {
        continuation: WorkflowContinuation,
        message: String,
    },
    /// A thread reached a safe point with an interrupt to service. The host persists both records
    /// in one transaction: the frozen thread, and the handler continuation now running beside it.
    Interrupted {
        suspended: WorkflowContinuation,
        handler: Box<WorkflowContinuation>,
        source: runinator_models::interrupt::InterruptSource,
    },
    /// A handler finished. The host retires it and applies `outcome` to the thread it suspended.
    InterruptResolved {
        handler: WorkflowContinuation,
        interrupted_continuation_id: uuid::Uuid,
        outcome: WorkflowInterruptOutcome,
    },
}

/// Resume a continuation after the host durably settled its sole outstanding effect.
///
/// `request` is the settled effect's own request. It is what lets the VM classify the arrival —
/// a timer elapsing, a park being resolved, a child run finishing — which is how the drive-matched
/// interrupt sources are detected without a host re-reading graph ancestry.
pub fn resume(
    module: &WorkflowModule,
    continuation: WorkflowContinuation,
    request: Option<&WorkflowEffectRequest>,
    result: Result<Value, WorkflowFailure>,
) -> WorkflowVmStep {
    resume_with_debug(module, continuation, request, result, None)
}

/// Resume with the run-scoped debugger configuration that was current when the host claimed this
/// continuation.
pub fn resume_with_debug(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    request: Option<&WorkflowEffectRequest>,
    result: Result<Value, WorkflowFailure>,
    debug: Option<&DebugConfig>,
) -> WorkflowVmStep {
    sync_debug_config(&mut continuation, debug);
    // Persisted effect settlement makes the row runnable so a scheduler can claim it, while the
    // effect id remains available for the durable host to load its immutable receipt.
    if !matches!(
        continuation.status,
        WorkflowContinuationStatus::Waiting | WorkflowContinuationStatus::Runnable
    ) || continuation.awaiting_effect_id.is_none()
    {
        return fail(
            continuation,
            "attempted to resume a continuation that is not waiting for an effect".into(),
        );
    }
    continuation.awaiting_effect_id = None;
    continuation.status = WorkflowContinuationStatus::Runnable;
    // Compensation is best-effort: both a successful and a failed undo settle the active entry,
    // then the next undo is issued. The original failure remains the terminal outcome once the
    // LIFO stack drains, regardless of which graph block happens to follow the failing node in
    // bytecode layout.
    if let Some(position) = continuation.frames.iter().position(
        |frame| matches!(frame, WorkflowFrame::Compensation(frame) if frame.active.is_some()),
    ) {
        let frame = match &mut continuation.frames[position] {
            WorkflowFrame::Compensation(frame) => frame,
            _ => unreachable!("compensation frame position was checked"),
        };
        frame.active = None;
        if let Some(request) = frame.pending.pop() {
            frame.active = Some(request.clone());
            return yield_effect(continuation, request);
        }
        continuation.frames.remove(position);
        if let Some(Value::String(message)) = continuation
            .locals
            .remove("__workflow_vm_compensation_failure")
        {
            return fail(continuation, message);
        }
    }
    // an arriving result is a safe point for the sources the VM can classify for itself. a handler
    // continuation is excluded: it may not be interrupted, and it may not fail the run.
    if let Some(source) = arrival_interrupt_source(request, &result)
        && let Some(step) = try_raise_detected(module, &continuation, source)
    {
        return step;
    }
    match result {
        Ok(value) => continuation.stack.push(value),
        Err(failure) => return handle_classified_failure(module, continuation, failure),
    }
    step_with_debug(module, continuation, debug)
}

/// Run a continuation until it reaches its next durable boundary.
pub fn step(module: &WorkflowModule, continuation: WorkflowContinuation) -> WorkflowVmStep {
    step_with_debug(module, continuation, None)
}

/// Run a continuation until its next durable boundary while honoring the supplied run-scoped
/// debugger configuration.
pub fn step_with_debug(
    module: &WorkflowModule,
    continuation: WorkflowContinuation,
    debug: Option<&DebugConfig>,
) -> WorkflowVmStep {
    step_until(module, continuation, debug, None)
}

/// Reconstruct a prefix without executing the checkpoint instruction. The returned `Joined`
/// boundary named `replay-checkpoint` is local-only and must never be persisted as a join.
pub fn step_to_replay_checkpoint(
    module: &WorkflowModule,
    continuation: WorkflowContinuation,
    checkpoint: usize,
) -> WorkflowVmStep {
    step_until(module, continuation, None, Some(checkpoint))
}

fn step_until(
    module: &WorkflowModule,
    mut continuation: WorkflowContinuation,
    debug: Option<&DebugConfig>,
    checkpoint: Option<usize>,
) -> WorkflowVmStep {
    sync_debug_config(&mut continuation, debug);
    if let Err(error) = module.ensure_supported() {
        return fail(continuation, error.to_string());
    }
    if let Err(error) = continuation.ensure_supported() {
        return fail(continuation, error.to_string());
    }
    if continuation.module_version != module.version {
        let continuation_version = continuation.module_version;
        return fail(
            continuation,
            format!(
                "continuation module version {} does not match module version {}",
                continuation_version, module.version
            ),
        );
    }
    if continuation.status != WorkflowContinuationStatus::Runnable {
        return fail(
            continuation,
            "attempted to step a non-runnable continuation".into(),
        );
    }

    if let Some(failure) = take_pending_debug_failure(&mut continuation) {
        return route_classified_failure(module, continuation, failure);
    }

    for _ in 0..MAX_INLINE_INSTRUCTIONS {
        if checkpoint == Some(continuation.instruction_pointer) {
            return WorkflowVmStep::Joined {
                continuation,
                join_key: "replay-checkpoint".into(),
                value: Value::Null,
            };
        }
        let Some(instruction) = module.instructions.get(continuation.instruction_pointer) else {
            return fail(
                continuation,
                "instruction pointer is outside the workflow module".into(),
            );
        };
        let outcome = match instruction {
            WorkflowInstruction::EnterNode { node_id } => stack::enter_node(continuation, node_id),
            WorkflowInstruction::Const { value } => stack::push_const(continuation, value),
            WorkflowInstruction::LoadLocal { name } => stack::load_local(continuation, name),
            WorkflowInstruction::StoreLocal { name } => stack::store_local(continuation, name),
            WorkflowInstruction::Pop => stack::pop(continuation),
            WorkflowInstruction::Jump { target } => stack::jump(continuation, target),
            WorkflowInstruction::JumpIfFalse { target } => {
                stack::jump_if_false(continuation, target)
            }
            WorkflowInstruction::Branch { branches, default } => {
                selectors::branch(continuation, branches, default)
            }
            WorkflowInstruction::Evaluate {
                module: invocation_module,
            } => selectors::evaluate(continuation, module, invocation_module),
            WorkflowInstruction::Select {
                kind,
                configuration,
                targets,
                default,
            } => selectors::select(continuation, module, kind, configuration, targets, default),
            WorkflowInstruction::NextLoop { loop_key } => {
                loops::next_loop(continuation, module, loop_key)
            }
            WorkflowInstruction::CheckInterrupt { handlers } => {
                interrupts::check_interrupt(continuation, module, handlers)
            }
            WorkflowInstruction::ResumeInterrupt { mode } => {
                interrupts::resume_interrupt(continuation, module, mode)
            }
            WorkflowInstruction::DebugBoundary { label } => {
                debug::debug_boundary(continuation, debug, label)
            }
            WorkflowInstruction::SetOutput {
                event_type,
                artifacts,
            } => output::set_output(continuation, module, event_type, artifacts),
            WorkflowInstruction::BeginLoop {
                loop_key,
                body,
                exit,
                max_iterations,
            } => loops::begin_loop(continuation, module, loop_key, body, exit, max_iterations),
            WorkflowInstruction::Reenter {
                reentry_key,
                target,
                exhausted,
                max_visits,
            } => loops::reenter(
                continuation,
                module,
                reentry_key,
                target,
                exhausted,
                max_visits,
            ),
            WorkflowInstruction::BeginTry {
                try_key,
                catch,
                on_timeout,
                on_reject,
                finally,
            } => structured::begin_try(
                continuation,
                module,
                try_key,
                catch,
                on_timeout,
                on_reject,
                finally,
            ),
            WorkflowInstruction::EndTry { try_key } => {
                structured::end_try(continuation, module, try_key)
            }
            WorkflowInstruction::RegisterCompensation {
                compensation_key: _,
                request,
            } => structured::register_compensation(continuation, request),
            WorkflowInstruction::BeginCompensation { resume } => {
                structured::begin_compensation(continuation, resume)
            }
            WorkflowInstruction::Effect { request } => effects::effect(continuation, request),
            WorkflowInstruction::Fork { targets, join_key } => {
                parallel::fork_instruction(continuation, targets, join_key)
            }
            WorkflowInstruction::Race {
                targets,
                race_key,
                winner,
            } => parallel::race(continuation, targets, race_key, winner),
            WorkflowInstruction::BeginMap {
                map_key,
                body,
                exit,
                concurrency,
            } => parallel::begin_map(continuation, map_key, body, exit, concurrency),
            WorkflowInstruction::Join {
                join_key,
                expected,
                mode,
            } => parallel::join(continuation, join_key, expected, mode),
            WorkflowInstruction::Return => transitions::return_value(continuation, module),
            WorkflowInstruction::Fail { message } => {
                transitions::fail_instruction(continuation, module, message)
            }
        };
        match outcome {
            Continue(next) => continuation = next,
            Break(boundary) => return boundary,
        }
    }
    fail(continuation, "workflow instruction budget exhausted".into())
}

mod context;
mod debug;
mod effects;
mod failure;
mod foreign_code;
mod interrupts;
mod loops;
mod output;
mod parallel;
mod selectors;
mod stack;
mod structured;
mod transitions;

use std::ops::ControlFlow::{self, Break, Continue};
type InstructionOutcome = ControlFlow<WorkflowVmStep, WorkflowContinuation>;

pub use interrupts::interrupt_handler_continuation;

#[cfg(test)]
mod interpreter_tests;
