//! debug behavior for the workflow interpreter.

use super::{InstructionOutcome, transitions};
use runinator_models::debug::should_break_at;
use runinator_models::workflow_state::{DebugConfig, DebugRuntime};
use runinator_models::workflow_vm::{WorkflowContinuation, WorkflowFailure, WorkflowFrame};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn sync_debug_config(
    continuation: &mut WorkflowContinuation,
    debug: Option<&DebugConfig>,
) {
    let Some(config) = debug.filter(|config| config.enabled) else {
        return;
    };
    if let Some(frame) = continuation
        .frames
        .iter_mut()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) => Some(frame),
            _ => None,
        })
    {
        frame.pause_on_failure = config.pause_on_failure;
    } else {
        continuation.frames.push(WorkflowFrame::Debug(
            runinator_models::workflow_vm::WorkflowDebugFrame {
                paused: false,
                step_requested: false,
                breakpoint: None,
                run_to_node_id: None,
                pending_failure: None,
                pause_on_failure: config.pause_on_failure,
                last_output: None,
                speculative: false,
            },
        ));
    }
}

pub(super) fn take_pending_debug_failure(
    continuation: &mut WorkflowContinuation,
) -> Option<WorkflowFailure> {
    continuation
        .frames
        .iter_mut()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) if !frame.paused => frame.pending_failure.take(),
            _ => None,
        })
}

pub(super) fn debug_boundary(
    mut continuation: WorkflowContinuation,
    debug: Option<&DebugConfig>,
    label: &Option<String>,
) -> InstructionOutcome {
    let one_shot = continuation
        .frames
        .iter()
        .rev()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) => frame.run_to_node_id.clone(),
            _ => None,
        });
    let breakpoint_matches = label.as_deref().is_some_and(|label| {
        debug.is_some_and(|config| {
            config.enabled
                && should_break_at(
                    config,
                    &DebugRuntime {
                        one_shot_breakpoint: one_shot.clone(),
                        ..DebugRuntime::default()
                    },
                    label,
                )
        })
    });
    let mut park_after_boundary = breakpoint_matches;
    if let Some(position) = continuation
        .frames
        .iter()
        .rposition(|frame| matches!(frame, WorkflowFrame::Debug(_)))
    {
        if let WorkflowFrame::Debug(frame) = &mut continuation.frames[position] {
            if label.as_ref() == frame.run_to_node_id.as_ref() {
                frame.run_to_node_id = None;
            }
            frame.breakpoint = label.clone();
            if frame.step_requested {
                frame.step_requested = false;
                frame.paused = true;
                park_after_boundary = true;
            } else if breakpoint_matches {
                frame.paused = true;
            }
        }
    } else {
        continuation.frames.push(WorkflowFrame::Debug(
            runinator_models::workflow_vm::WorkflowDebugFrame {
                paused: breakpoint_matches,
                step_requested: false,
                breakpoint: label.clone(),
                run_to_node_id: None,
                pending_failure: None,
                pause_on_failure: debug.is_some_and(|config| config.pause_on_failure),
                last_output: None,
                speculative: false,
            },
        ));
    }
    continuation.instruction_pointer += 1;
    if park_after_boundary {
        return Break(transitions::pause(continuation, "debug-step"));
    }
    Continue(continuation)
}
