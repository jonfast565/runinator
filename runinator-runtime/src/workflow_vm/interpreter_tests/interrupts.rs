//! interrupts interpreter regressions.

use super::*;

#[test]
fn a_handler_id_is_stable_per_occurrence_but_distinct_across_them() {
    // the engine's retry path starts one handler per attempt. the id must be stable enough that
    // a redelivered result inserts nothing, and distinct enough that attempt 2 still gets a
    // handler of its own.
    let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
    let continuation = WorkflowContinuation::start(Uuid::now_v7(), module.version);
    let build = |discriminator: &str| {
        interrupt_handler_continuation(
            &module,
            &continuation,
            InterruptSource::Retry,
            Value::Null,
            0,
            discriminator,
        )
        .id
    };

    assert_eq!(build("attempt:1"), build("attempt:1"));
    assert_ne!(build("attempt:1"), build("attempt:2"));
}

#[test]
fn a_handler_reads_what_raised_it_and_leaves_the_thread_alone() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Return]);
    let continuation = WorkflowContinuation::start(Uuid::now_v7(), module.version);
    let before = continuation.status;
    let handler = interrupt_handler_continuation(
        &module,
        &continuation,
        InterruptSource::Retry,
        runinator_models::json!({ "next_attempt": 2 }),
        0,
        "attempt:2",
    );

    assert_eq!(handler.parent_id, Some(continuation.id));
    assert_eq!(handler.locals["interrupt"]["source"], Value::from("retry"));
    assert_eq!(handler.locals["interrupt"]["payload"]["next_attempt"], 2);
    // building a handler decides nothing about the interrupted thread; the retry path relies on
    // that, because a suspended thread could never be settled by its retried effect.
    assert_eq!(continuation.status, before);
}

#[test]
fn a_requested_interrupt_freezes_its_thread_and_a_handler_runs_beside_it() {
    let module = interrupt_module();
    let mut continuation = continuation();
    continuation.pending_interrupt =
        Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
            id: uuid::Uuid::now_v7(),
            source: InterruptSource::External,
            payload: Value::from("stop"),
        });

    let WorkflowVmStep::Interrupted {
        suspended, handler, ..
    } = step(&module, continuation)
    else {
        panic!("a requested interrupt must suspend its thread");
    };
    assert_eq!(suspended.status, WorkflowContinuationStatus::Suspended);
    // consumed by the drive that decided about it, so it cannot fire again later.
    assert_eq!(suspended.pending_interrupt, None);
    assert_eq!(handler.instruction_pointer, 4);
    assert_eq!(handler.parent_id, Some(suspended.id));
    assert_eq!(
        handler
            .locals
            .get("interrupt")
            .and_then(|v| v.get("source")),
        Some(&Value::from("external"))
    );

    let WorkflowVmStep::InterruptResolved {
        interrupted_continuation_id,
        outcome,
        handler,
    } = step(&module, *handler)
    else {
        panic!("the handler must hand control back");
    };
    assert_eq!(interrupted_continuation_id, suspended.id);
    assert_eq!(handler.status, WorkflowContinuationStatus::Succeeded);
    // `resume` continues after the safe point, never at it — otherwise the same interrupt
    // would be re-examined on the very next drive.
    assert_eq!(
        outcome,
        WorkflowInterruptOutcome::Resume {
            instruction_pointer: 2
        }
    );
}

#[test]
fn a_timer_request_selects_its_own_handler_and_occurrence() {
    let mut module = interrupt_module();
    module.instructions[1] = WorkflowInstruction::CheckInterrupt {
        handlers: vec![
            runinator_models::workflow_vm::WorkflowVmInterruptHandler {
                source: InterruptSource::Timer,
                target: 4,
                timer_id: Some("fast".into()),
                interval_seconds: Some(30),
            },
            runinator_models::workflow_vm::WorkflowVmInterruptHandler {
                source: InterruptSource::Timer,
                target: 5,
                timer_id: Some("slow".into()),
                interval_seconds: Some(300),
            },
        ],
    };
    let mut continuation = continuation();
    continuation.pending_interrupt =
        Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
            id: uuid::Uuid::now_v7(),
            source: InterruptSource::Timer,
            payload: runinator_models::json!({ "timer_id": "slow", "due_at": 600 }),
        });

    let WorkflowVmStep::Interrupted { handler, .. } = step(&module, continuation) else {
        panic!("a declared timer occurrence must raise its matching handler");
    };
    assert_eq!(handler.instruction_pointer, 5);
    assert_eq!(
        handler.locals["interrupt"]["payload"]["timer_id"],
        Value::from("slow")
    );
}

#[test]
fn a_handler_mode_picks_where_the_frozen_thread_lands() {
    let module = interrupt_module();
    for (mode, expected) in [
        (InterruptMode::Resume, Some(2usize)),
        (InterruptMode::Restart, Some(0)),
        (InterruptMode::Continue, Some(3)),
        (InterruptMode::Fail, None),
    ] {
        let mut continuation = continuation();
        continuation.pending_interrupt =
            Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
                id: uuid::Uuid::now_v7(),
                source: InterruptSource::External,
                payload: Value::Null,
            });
        let WorkflowVmStep::Interrupted { handler, .. } = step(&module, continuation) else {
            panic!("expected a suspension");
        };
        let WorkflowVmStep::InterruptResolved { outcome, .. } =
            resolve_interrupt(&module, *handler, mode)
        else {
            panic!("{mode:?} must resolve the interrupt");
        };
        match (expected, outcome) {
            (
                Some(ip),
                WorkflowInterruptOutcome::Resume {
                    instruction_pointer,
                },
            ) => {
                assert_eq!(instruction_pointer, ip, "{mode:?}")
            }
            // a handler can settle the interrupted node failed; it can never fail the run.
            (None, WorkflowInterruptOutcome::Fail { .. }) => {}
            (expected, outcome) => panic!("{mode:?} gave {outcome:?}, wanted {expected:?}"),
        }
    }
}

#[test]
fn a_source_nobody_declared_is_dropped_rather_than_left_pending() {
    let module = interrupt_module();
    let mut continuation = continuation();
    continuation.pending_interrupt =
        Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
            id: uuid::Uuid::now_v7(),
            source: InterruptSource::Child,
            payload: Value::Null,
        });
    let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation) else {
        panic!("an unhandled source must not stop the drive");
    };
    assert_eq!(continuation.pending_interrupt, None);
}

#[test]
fn a_settled_effect_raises_the_source_it_represents() {
    let mut module = interrupt_module();
    module.instructions[1] = WorkflowInstruction::CheckInterrupt {
        handlers: Vec::new(),
    };
    module.interrupt_handlers = vec![runinator_models::workflow_vm::WorkflowVmInterruptHandler {
        source: InterruptSource::Wake,
        target: 4,
        timer_id: None,
        interval_seconds: None,
    }];
    let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
        panic!("the timer must yield");
    };
    let request = WorkflowEffectRequest::TimerDelay { seconds: 1 };
    let WorkflowVmStep::Interrupted { source, .. } =
        resume(&module, continuation, Some(&request), Ok(Value::Null))
    else {
        panic!("an elapsed timer with a `wake` handler must raise it");
    };
    assert_eq!(source, InterruptSource::Wake);
}

#[test]
fn a_handler_that_breaks_gives_the_frozen_thread_back_instead_of_failing_the_run() {
    let mut module = interrupt_module();
    module.instructions[4] = WorkflowInstruction::Fail {
        message: "handler blew up".into(),
    };
    let mut continuation = continuation();
    continuation.pending_interrupt =
        Some(runinator_models::workflow_vm::WorkflowPendingInterrupt {
            id: uuid::Uuid::now_v7(),
            source: InterruptSource::External,
            payload: Value::Null,
        });
    let WorkflowVmStep::Interrupted { handler, .. } = step(&module, continuation) else {
        panic!("expected a suspension");
    };
    let WorkflowVmStep::InterruptResolved { outcome, .. } = step(&module, *handler) else {
        panic!("a broken handler must still hand control back, not fail the run");
    };
    assert_eq!(
        outcome,
        WorkflowInterruptOutcome::Resume {
            instruction_pointer: 2
        }
    );
}
