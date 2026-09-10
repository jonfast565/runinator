//! structured interpreter regressions.

use super::*;

#[test]
fn a_timeout_takes_the_on_timeout_edge_and_a_plain_failure_the_catch() {
    // the shape the compiler emits for a node carrying both `on_failure` and `on_timeout`:
    // one guard whose classified target is preferred over `catch`.
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::BeginTry {
            try_key: "call#edge".into(),
            catch: Some(5),
            on_timeout: Some(7),
            on_reject: None,
            finally: None,
        },
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
        },
        WorkflowInstruction::EndTry {
            try_key: "call#edge".into(),
        },
        WorkflowInstruction::Const {
            value: Value::from("ok"),
        },
        WorkflowInstruction::Return,
        WorkflowInstruction::EndTry {
            try_key: "call#edge".into(),
        },
        WorkflowInstruction::Fail {
            message: "recovered".into(),
        },
        WorkflowInstruction::EndTry {
            try_key: "call#edge".into(),
        },
        WorkflowInstruction::Fail {
            message: "slow".into(),
        },
    ]);

    for (kind, expected) in [
        (WorkflowFailureKind::TimedOut, "slow"),
        (WorkflowFailureKind::Failed, "recovered"),
        // no `on_reject` edge was compiled, so a rejection falls back to `catch`.
        (WorkflowFailureKind::Rejected, "recovered"),
    ] {
        let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
            panic!("the guarded effect must yield");
        };
        let WorkflowVmStep::Failed { message, .. } = resume(
            &module,
            continuation,
            None,
            Err(WorkflowFailure::new(kind, "boom")),
        ) else {
            panic!("{kind:?} must reach a terminal");
        };
        assert_eq!(message, expected, "{kind:?} took the wrong edge");
    }
}

#[test]
fn loop_opcode_rejects_a_non_array_initial_value() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::BeginLoop {
        loop_key: "items".into(),
        body: 0,
        exit: 0,
        max_iterations: None,
    }]);
    let WorkflowVmStep::Failed { message, .. } = step(&module, continuation()) else {
        panic!("expected an unsupported-opcode failure");
    };
    assert!(message.contains("needs an array value"));
}

#[test]
fn loop_frame_survives_reentry_and_collects_results_in_order() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Const {
            value: Value::Array(vec![Value::from(1), Value::from(2)]),
        },
        WorkflowInstruction::BeginLoop {
            loop_key: "items".into(),
            body: 5,
            exit: 4,
            max_iterations: None,
        },
        WorkflowInstruction::Fail {
            message: "unreachable".into(),
        },
        WorkflowInstruction::Fail {
            message: "unreachable".into(),
        },
        WorkflowInstruction::Return,
        WorkflowInstruction::Const {
            value: Value::String("item".into()),
        },
        // This mirrors the compiler's item expression on a graph re-entry. BeginLoop
        // discards it while retaining the body result below it.
        WorkflowInstruction::Const {
            value: Value::Array(vec![Value::from(1), Value::from(2)]),
        },
        WorkflowInstruction::Jump { target: 1 },
    ]);
    let WorkflowVmStep::Complete {
        value,
        continuation,
    } = step(&module, continuation())
    else {
        panic!("loop should finish");
    };
    assert_eq!(
        value,
        Value::Array(vec![
            Value::String("item".into()),
            Value::String("item".into())
        ])
    );
    assert!(
        continuation
            .frames
            .iter()
            .all(|frame| !matches!(frame, WorkflowFrame::Loop(_)))
    );
}

#[test]
fn failure_unwinds_compensation_before_the_terminal_failure() {
    let action = WorkflowEffectRequest::Timer { due_at: 1 };
    let compensation = WorkflowEffectRequest::Timer { due_at: 2 };
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect { request: action },
        WorkflowInstruction::RegisterCompensation {
            compensation_key: "charge".into(),
            request: compensation.clone(),
        },
        WorkflowInstruction::Fail {
            message: "charge failed".into(),
        },
        WorkflowInstruction::BeginCompensation { resume: None },
    ]);
    let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
        panic!("main action should yield")
    };
    let WorkflowVmStep::Yield {
        continuation,
        request,
        ..
    } = resume(&module, continuation, None, Ok(Value::Null))
    else {
        panic!("compensation should yield")
    };
    assert_eq!(*request, compensation);
    let WorkflowVmStep::Failed {
        message,
        continuation,
    } = resume(&module, continuation, None, Ok(Value::Null))
    else {
        panic!("failure should survive compensation")
    };
    assert_eq!(message, "charge failed");
    assert_eq!(continuation.status, WorkflowContinuationStatus::Failed);
}

#[test]
fn try_catch_finally_unwinds_through_the_same_continuation() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::BeginTry {
            try_key: "guard".into(),
            catch: Some(4),
            on_timeout: None,
            on_reject: None,
            finally: Some(6),
        },
        WorkflowInstruction::Jump { target: 3 },
        WorkflowInstruction::Return,
        WorkflowInstruction::Fail {
            message: "body failed".into(),
        },
        WorkflowInstruction::Const {
            value: Value::String("caught".into()),
        },
        WorkflowInstruction::Jump { target: 0 },
        WorkflowInstruction::Const {
            value: Value::String("finally".into()),
        },
        WorkflowInstruction::Jump { target: 0 },
    ]);
    let WorkflowVmStep::Complete {
        value,
        continuation,
    } = step(&module, continuation())
    else {
        panic!("catch/finally should complete")
    };
    assert_eq!(value, Value::String("finally".into()));
    assert!(
        continuation
            .frames
            .iter()
            .all(|frame| !matches!(frame, WorkflowFrame::Try(_)))
    );
}

#[test]
fn loop_reentry_and_next_instruction_apply_identical_limits_and_results() {
    for limit in [Some(0), Some(1), Some(2), Some(3), Some(4), None] {
        let items = Value::Array(vec![Value::from(10), Value::from(20), Value::from(30)]);
        let direct = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: items.clone(),
            },
            WorkflowInstruction::BeginLoop {
                loop_key: "items".into(),
                body: 2,
                exit: 4,
                max_iterations: limit,
            },
            WorkflowInstruction::LoadLocal {
                name: "items.index".into(),
            },
            WorkflowInstruction::NextLoop {
                loop_key: "items".into(),
            },
            WorkflowInstruction::Return,
        ]);
        let reentry = WorkflowModule::new(vec![
            WorkflowInstruction::Const {
                value: items.clone(),
            },
            WorkflowInstruction::BeginLoop {
                loop_key: "items".into(),
                body: 2,
                exit: 5,
                max_iterations: limit,
            },
            WorkflowInstruction::LoadLocal {
                name: "items.index".into(),
            },
            WorkflowInstruction::Const { value: items },
            WorkflowInstruction::Jump { target: 1 },
            WorkflowInstruction::Return,
        ]);
        let WorkflowVmStep::Complete {
            continuation: direct_state,
            value: direct_value,
        } = step(&direct, continuation())
        else {
            panic!("next-loop program must complete");
        };
        let WorkflowVmStep::Complete {
            continuation: reentry_state,
            value: reentry_value,
        } = step(&reentry, continuation())
        else {
            panic!("reentry program must complete");
        };
        let count = limit.unwrap_or(3).min(3);
        assert_eq!(
            direct_value,
            Value::Array((0..count).map(|index| Value::from(index as i64)).collect())
        );
        assert_eq!(direct_value, reentry_value);
        assert_eq!(direct_state.locals, reentry_state.locals);
        assert!(direct_state.frames.is_empty());
        assert!(reentry_state.frames.is_empty());
    }
}
