//! execution interpreter regressions.

use super::*;

#[test]
fn compiled_linear_graph_reaches_the_same_terminal_result() {
    let module = compile_workflow_module(&WorkflowDefinition {
        output_type: Default::default(),
        id: None,
        name: "linear".into(),
        key: None,
        namespace: None,
        org_id: None,
        version: Default::default(),
        enabled: true,
        input_type: Default::default(),
        definition: WorkflowGraph {
            start: Some("start".into()),
            nodes: vec![
                vm_node("start", WorkflowNodeKind::Start, Some("end")),
                vm_node("end", WorkflowNodeKind::End, None),
            ],
            ..Default::default()
        },
        created_at: None,
        updated_at: None,
    })
    .unwrap();

    let WorkflowVmStep::Complete {
        value,
        continuation,
    } = step(&module, continuation())
    else {
        panic!("expected the compiled linear graph to complete");
    };
    assert_eq!(value, Value::Null);
    assert_eq!(continuation.status, WorkflowContinuationStatus::Succeeded);
}

#[test]
fn toggle_selector_selects_a_boolean_target() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Select {
            kind: WorkflowNodeKind::Toggle,
            configuration: serde_json::json!({
                "parameters": { "value": true }
            })
            .into(),
            targets: vec![1, 3],
            default: None,
        },
        WorkflowInstruction::Const {
            value: Value::String("on".into()),
        },
        WorkflowInstruction::Jump { target: 4 },
        WorkflowInstruction::Const {
            value: Value::String("off".into()),
        },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Complete { value, .. } = step(&module, continuation()) else {
        panic!("toggle should select a branch");
    };
    assert_eq!(value, Value::String("on".into()));
}

#[test]
fn output_and_debug_opcodes_are_executable() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Const {
            value: Value::String("result".into()),
        },
        WorkflowInstruction::SetOutput {
            event_type: Some("finished".into()),
            artifacts: vec![],
        },
        WorkflowInstruction::DebugBoundary {
            label: Some("after-output".into()),
        },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Complete {
        continuation,
        value,
    } = step(&module, continuation())
    else {
        panic!("output and debug instructions should not be unsupported");
    };
    assert_eq!(value, Value::String("result".into()));
    assert_eq!(
        continuation
            .locals
            .get("__workflow_vm_output")
            .and_then(|value| value.get("event_type")),
        Some(&Value::String("finished".into()))
    );
    assert!(continuation.frames.iter().any(|frame| matches!(frame, WorkflowFrame::Debug(frame) if frame.breakpoint.as_deref() == Some("after-output"))));
}

#[test]
fn configured_breakpoint_parks_before_the_matching_boundary() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::DebugBoundary {
            label: Some("pass".into()),
        },
        WorkflowInstruction::DebugBoundary {
            label: Some("stop".into()),
        },
        WorkflowInstruction::Return,
    ]);
    let config = DebugConfig {
        enabled: true,
        mode: Some(runinator_models::workflow_state::DebugMode::Breakpoints),
        breakpoints: vec!["stop".into()],
        pause_on_failure: false,
    };

    let WorkflowVmStep::Joined {
        continuation,
        join_key,
        ..
    } = step_with_debug(&module, continuation(), Some(&config))
    else {
        panic!("matching breakpoint should park the continuation");
    };

    assert_eq!(join_key, "debug-step");
    assert_eq!(continuation.status, WorkflowContinuationStatus::Paused);
    assert!(continuation.operator_paused);
    assert!(continuation.frames.iter().any(|frame| matches!(frame, WorkflowFrame::Debug(frame) if frame.paused && frame.breakpoint.as_deref() == Some("stop"))));
}

#[test]
fn empty_breakpoint_set_runs_through_debug_boundaries() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::DebugBoundary {
            label: Some("pass".into()),
        },
        WorkflowInstruction::Return,
    ]);
    let config = DebugConfig {
        enabled: true,
        mode: Some(runinator_models::workflow_state::DebugMode::Breakpoints),
        breakpoints: vec![],
        pause_on_failure: false,
    };

    let WorkflowVmStep::Complete { .. } = step_with_debug(&module, continuation(), Some(&config))
    else {
        panic!("an unset breakpoint should not park the continuation");
    };
}

#[test]
fn run_to_target_is_one_shot_and_branch_local() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::DebugBoundary {
            label: Some("start".into()),
        },
        WorkflowInstruction::DebugBoundary {
            label: Some("target".into()),
        },
        WorkflowInstruction::Return,
    ]);
    let mut config = DebugConfig {
        enabled: true,
        mode: Some(runinator_models::workflow_state::DebugMode::Breakpoints),
        breakpoints: vec!["start".into()],
        pause_on_failure: false,
    };
    let WorkflowVmStep::Joined {
        mut continuation, ..
    } = step_with_debug(&module, continuation(), Some(&config))
    else {
        panic!("start breakpoint should park");
    };
    continuation.status = WorkflowContinuationStatus::Runnable;
    continuation.operator_paused = false;
    let frame = continuation
        .frames
        .iter_mut()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) => Some(frame),
            _ => None,
        })
        .unwrap();
    frame.paused = false;
    frame.run_to_node_id = Some("target".into());
    config.breakpoints.clear();

    let WorkflowVmStep::Joined { continuation, .. } =
        step_with_debug(&module, continuation, Some(&config))
    else {
        panic!("run-to target should park");
    };
    assert!(continuation.frames.iter().any(|frame| matches!(frame,
        WorkflowFrame::Debug(frame) if frame.paused && frame.breakpoint.as_deref() == Some("target") && frame.run_to_node_id.is_none()
    )));
}

#[test]
fn pause_on_failure_stops_once_before_error_routing() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Fail {
        message: "boom".into(),
    }]);
    let config = DebugConfig {
        enabled: true,
        mode: Some(runinator_models::workflow_state::DebugMode::Breakpoints),
        breakpoints: vec![],
        pause_on_failure: true,
    };
    let WorkflowVmStep::Joined {
        mut continuation,
        join_key,
        ..
    } = step_with_debug(&module, continuation(), Some(&config))
    else {
        panic!("failure should pause before routing");
    };
    assert_eq!(join_key, "debug-failure");
    continuation.status = WorkflowContinuationStatus::Runnable;
    continuation.operator_paused = false;
    let frame = continuation
        .frames
        .iter_mut()
        .find_map(|frame| match frame {
            WorkflowFrame::Debug(frame) => Some(frame),
            _ => None,
        })
        .unwrap();
    frame.paused = false;

    let WorkflowVmStep::Failed {
        message,
        continuation,
    } = step_with_debug(&module, continuation, Some(&config))
    else {
        panic!("resumed failure should route instead of pausing twice");
    };
    assert_eq!(message, "boom");
    assert!(continuation.frames.iter().any(|frame| matches!(frame,
        WorkflowFrame::Debug(frame) if frame.pending_failure.is_none()
    )));
}

fn vm_node(id: &str, kind: WorkflowNodeKind, next: Option<&str>) -> WorkflowNode {
    WorkflowNode {
        id: id.into(),
        kind,
        skipped: false,
        locked: false,
        action: None,
        parameters: Default::default(),
        wait: Default::default(),
        condition: Default::default(),
        transitions: WorkflowTransitions {
            next: next.map(WorkflowNodeRef::new),
            ..Default::default()
        },
        retry: Default::default(),
        timeout_seconds: None,
        max_iterations: None,
        subflow_id: None,
        subflow: Default::default(),
        reentry: Default::default(),
        compensation: None,
    }
}

#[test]
fn inline_dispatch_preserves_the_instruction_budget() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Jump { target: 0 }]);
    let WorkflowVmStep::Failed {
        continuation,
        message,
    } = step(&module, continuation())
    else {
        panic!("an inline cycle must exhaust the instruction budget");
    };
    assert_eq!(message, "workflow instruction budget exhausted");
    assert_eq!(continuation.status, WorkflowContinuationStatus::Failed);
    assert_eq!(continuation.next_effect_sequence, 0);
}

#[test]
fn replay_checkpoint_stops_before_dispatching_the_checkpoint_instruction() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Const {
            value: Value::from(7),
        },
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
        },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Joined {
        continuation: checkpoint,
        join_key,
        ..
    } = step_to_replay_checkpoint(&module, continuation(), 1)
    else {
        panic!("replay must stop before the effect");
    };
    assert_eq!(join_key, "replay-checkpoint");
    assert_eq!(checkpoint.instruction_pointer, 1);
    assert_eq!(checkpoint.stack, vec![Value::from(7)]);
    assert_eq!(checkpoint.status, WorkflowContinuationStatus::Runnable);
    assert_eq!(checkpoint.next_effect_sequence, 0);
    assert!(checkpoint.awaiting_effect_id.is_none());
    assert!(matches!(
        step(&module, checkpoint),
        WorkflowVmStep::Yield { sequence: 0, .. }
    ));
}
