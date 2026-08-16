//! covers the resumable vm: in-process evaluation, the four step outcomes, yield/resume across a
//! serialization boundary, local-call recording, and the frame limits.

use super::*;
use runinator_models::invocation::{CallPolicy, InvocationFunction};
use runinator_models::value::Map;

fn catalog() -> CallableCatalog {
    CallableCatalog::builtin()
}

fn konst(n: i64) -> InvocationInstruction {
    InvocationInstruction::Const {
        value: Value::from(n),
    }
}

fn text(value: &str) -> InvocationInstruction {
    InvocationInstruction::Const {
        value: Value::from(value),
    }
}

fn call(name: &str, argc: usize) -> InvocationInstruction {
    InvocationInstruction::Call {
        target: CallableTarget::Intrinsic {
            name: name.to_string(),
        },
        argc,
        names: Vec::new(),
        policy: None,
    }
}

fn provider_call(provider: &str, function: &str, argc: usize) -> InvocationInstruction {
    InvocationInstruction::Call {
        target: CallableTarget::Provider {
            provider: provider.to_string(),
            function: function.to_string(),
        },
        argc,
        names: Vec::new(),
        policy: None,
    }
}

fn run_entry(instructions: Vec<InvocationInstruction>) -> InvocationStep {
    let module = InvocationModule::new(InvocationProgram::new(instructions));
    let context = Value::Null;
    let catalog = catalog();
    start(&module, &VmEnv::pure(&context, &catalog))
}

fn expect_value(step: InvocationStep) -> Value {
    match step {
        InvocationStep::Complete { value } => value,
        other => panic!("expected completion, got {other:?}"),
    }
}

#[test]
fn a_constant_returns_itself() {
    let value = expect_value(run_entry(vec![konst(7), InvocationInstruction::Return]));
    assert_eq!(value, Value::from(7i64));
}

#[test]
fn falling_off_the_end_returns_null() {
    // an author writing a block with no `return` means null, not an error.
    let value = expect_value(run_entry(vec![konst(1), InvocationInstruction::Pop]));
    assert_eq!(value, Value::Null);
}

#[test]
fn locals_bind_and_load() {
    let value = expect_value(run_entry(vec![
        konst(41),
        InvocationInstruction::StoreLocal {
            name: "x".to_string(),
        },
        InvocationInstruction::LoadLocal {
            name: "x".to_string(),
        },
        konst(1),
        call("add", 2),
        InvocationInstruction::Return,
    ]));
    assert_eq!(value, Value::from(42i64));
}

#[test]
fn a_pure_intrinsic_folds_in_process() {
    let value = expect_value(run_entry(vec![
        text("hello"),
        call("upper", 1),
        InvocationInstruction::Return,
    ]));
    assert_eq!(value, Value::from("HELLO"));
}

#[test]
fn an_unknown_local_reads_as_null() {
    // matching the tree evaluator, which resolves `let.x` through the same missing-path rule every
    // other reference uses. reachable whenever a binding sits on a branch that did not run.
    let step = run_entry(vec![
        InvocationInstruction::LoadLocal {
            name: "nope".to_string(),
        },
        InvocationInstruction::Return,
    ]);
    assert!(matches!(
        step,
        InvocationStep::Complete { value: Value::Null }
    ));
}

#[test]
fn a_stack_underflow_is_an_error_not_a_panic() {
    let step = run_entry(vec![InvocationInstruction::Return]);
    assert!(matches!(step, InvocationStep::Failed { .. }));
}

#[test]
fn jump_if_false_takes_the_branch_on_a_falsy_value() {
    // if false { 1 } else { 2 }
    let value = expect_value(run_entry(vec![
        InvocationInstruction::Const {
            value: Value::Bool(false),
        },
        InvocationInstruction::JumpIfFalse { target: 4 },
        konst(1),
        InvocationInstruction::Return,
        konst(2),
        InvocationInstruction::Return,
    ]));
    assert_eq!(value, Value::from(2i64));
}

#[test]
fn truthiness_matches_the_condition_evaluator() {
    // javascript-like, because that is the rule the tree evaluator already applied to a `{value: x}`
    // condition and to a conditional expression. an earlier version of this test asserted the
    // opposite — that only null and `false` are falsy — which is the rule behind the `not`/`and`/`or`
    // intrinsics, a different surface. adopting it here inverted branches on `0` and on empty
    // collections, silently and undetectably.
    assert!(!truthy(&Value::from(0i64)));
    assert!(!truthy(&Value::from("")));
    assert!(!truthy(&Value::Array(Vec::new())));
    assert!(!truthy(&Value::Null));
    assert!(!truthy(&Value::Bool(false)));
    assert!(truthy(&Value::from(1i64)));
    assert!(truthy(&Value::from("x")));
    assert!(truthy(&Value::Bool(true)));
}

#[test]
fn goto_is_reported_as_its_own_outcome() {
    let step = run_entry(vec![InvocationInstruction::Goto {
        target: "done".to_string(),
    }]);
    match step {
        InvocationStep::Goto { target } => assert_eq!(target, "done"),
        other => panic!("expected a goto, got {other:?}"),
    }
}

#[test]
fn a_provider_call_yields_rather_than_running() {
    let step = run_entry(vec![
        text("main"),
        provider_call("github", "deploy", 1),
        InvocationInstruction::Return,
    ]);
    match step {
        InvocationStep::Yield { effect, .. } => {
            assert_eq!(effect.target.display_name(), "github.deploy");
            assert_eq!(effect.args, vec![Value::from("main")]);
            assert_eq!(effect.sequence, 0);
        }
        other => panic!("expected a yield, got {other:?}"),
    }
}

#[test]
fn a_yielded_call_resumes_with_its_result() {
    let module = InvocationModule::new(InvocationProgram::new(vec![
        text("main"),
        provider_call("github", "deploy", 1),
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let env = VmEnv::pure(&context, &catalog);
    let InvocationStep::Yield { continuation, .. } = start(&module, &env) else {
        panic!("expected a yield");
    };
    let step = resume(
        &module,
        *continuation,
        InvocationEffectResult::ok(Value::from("deployed")),
        &env,
    );
    assert_eq!(expect_value(step), Value::from("deployed"));
}

#[test]
fn a_continuation_survives_serialization_between_yield_and_resume() {
    // this is the property the whole design rests on: nothing execution needs may live in rust
    // call frames across the suspension.
    let module = InvocationModule::new(InvocationProgram::new(vec![
        text("main"),
        provider_call("github", "deploy", 1),
        call("upper", 1),
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let env = VmEnv::pure(&context, &catalog);
    let InvocationStep::Yield { continuation, .. } = start(&module, &env) else {
        panic!("expected a yield");
    };

    let encoded = serde_json::to_string(&*continuation).expect("serialize");
    let decoded: InvocationContinuation = serde_json::from_str(&encoded).expect("deserialize");

    let step = resume(
        &module,
        decoded,
        InvocationEffectResult::ok(Value::from("deployed")),
        &env,
    );
    // the post-call instruction still ran, against the resumed stack.
    assert_eq!(expect_value(step), Value::from("DEPLOYED"));
}

#[test]
fn a_failed_call_fails_the_invocation() {
    let module = InvocationModule::new(InvocationProgram::new(vec![
        provider_call("github", "deploy", 0),
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let env = VmEnv::pure(&context, &catalog);
    let InvocationStep::Yield { continuation, .. } = start(&module, &env) else {
        panic!("expected a yield");
    };
    let step = resume(
        &module,
        *continuation,
        InvocationEffectResult::failed("exhausted"),
        &env,
    );
    match step {
        InvocationStep::Failed { message } => assert!(message.contains("exhausted")),
        other => panic!("expected failure, got {other:?}"),
    }
}

#[test]
fn resuming_a_continuation_that_was_not_awaiting_is_rejected() {
    let module = InvocationModule::new(InvocationProgram::new(vec![konst(1)]));
    let context = Value::Null;
    let catalog = catalog();
    let env = VmEnv::pure(&context, &catalog);
    let step = resume(
        &module,
        InvocationContinuation::start(),
        InvocationEffectResult::ok(Value::Null),
        &env,
    );
    assert!(matches!(step, InvocationStep::Failed { .. }));
}

#[test]
fn a_module_from_a_future_ir_version_is_refused() {
    let mut module = InvocationModule::new(InvocationProgram::new(vec![konst(1)]));
    module.version = 999;
    let context = Value::Null;
    let catalog = catalog();
    let step = start(&module, &VmEnv::pure(&context, &catalog));
    assert!(matches!(step, InvocationStep::Failed { .. }));
}

#[test]
fn calls_over_a_secret_placeholder_always_yield() {
    // the whole point: folding `upper(secret://…)` in process would uppercase the placeholder.
    let step = run_entry(vec![
        text("secret://aws/key"),
        call("upper", 1),
        InvocationInstruction::Return,
    ]);
    match step {
        InvocationStep::Yield { effect, .. } => {
            assert_eq!(effect.target.display_name(), "upper");
        }
        other => panic!("expected a yield over a secret, got {other:?}"),
    }
}

#[test]
fn an_ordinary_string_still_folds_in_process() {
    let value = expect_value(run_entry(vec![
        text("not a secret"),
        call("upper", 1),
        InvocationInstruction::Return,
    ]));
    assert_eq!(value, Value::from("NOT A SECRET"));
}

// a library that answers the local intrinsics with fixed values, so recording is observable.
struct FixedLocals;

impl IntrinsicLibrary for FixedLocals {
    fn call(&self, name: &str, _args: &[Value]) -> Result<Value, WorkflowValidationError> {
        match name {
            "now" => Ok(Value::from("2026-08-15T00:00:00Z")),
            "uuid" => Ok(Value::from("11111111-1111-1111-1111-111111111111")),
            other => Err(WorkflowValidationError::UnknownIntrinsic(other.to_string())),
        }
    }

    fn knows(&self, name: &str) -> bool {
        matches!(name, "now" | "uuid")
    }

    fn is_pure(&self, _name: &str) -> bool {
        false
    }
}

#[test]
fn a_local_intrinsic_folds_in_process_rather_than_yielding() {
    let module = InvocationModule::new(InvocationProgram::new(vec![
        call("now", 0),
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let locals = FixedLocals;
    let env = VmEnv::with_locals(&context, &catalog, &locals);
    // no broker round trip for a clock read.
    assert_eq!(
        expect_value(start(&module, &env)),
        Value::from("2026-08-15T00:00:00Z")
    );
}

#[test]
fn a_local_intrinsic_is_recorded_and_replayed_on_resume() {
    // `now` before a yield must report the same instant after the resume, or a run would tell a
    // different story each time it was inspected.
    let module = InvocationModule::new(InvocationProgram::new(vec![
        call("now", 0),
        InvocationInstruction::StoreLocal {
            name: "t".to_string(),
        },
        provider_call("svc", "work", 0),
        InvocationInstruction::Pop,
        InvocationInstruction::LoadLocal {
            name: "t".to_string(),
        },
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let locals = FixedLocals;
    let env = VmEnv::with_locals(&context, &catalog, &locals);

    let InvocationStep::Yield { continuation, .. } = start(&module, &env) else {
        panic!("expected a yield");
    };
    // the observation was recorded in the continuation.
    assert_eq!(continuation.recorded.len(), 1);
    assert_eq!(continuation.recorded[0].name, "now");

    let step = resume(
        &module,
        *continuation,
        InvocationEffectResult::ok(Value::Null),
        &env,
    );
    assert_eq!(expect_value(step), Value::from("2026-08-15T00:00:00Z"));
}

#[test]
fn a_local_intrinsic_is_refused_in_a_pure_only_position() {
    // with no local library, observing the host is an error rather than a silent answer.
    let step = run_entry(vec![call("now", 0), InvocationInstruction::Return]);
    assert!(matches!(step, InvocationStep::Failed { .. }));
}

#[test]
fn a_module_function_runs_in_its_own_frame() {
    let mut module = InvocationModule::new(InvocationProgram::new(vec![
        konst(20),
        InvocationInstruction::Call {
            target: CallableTarget::Local {
                name: "double".to_string(),
            },
            argc: 1,
            names: Vec::new(),
            policy: None,
        },
        InvocationInstruction::Return,
    ]));
    module.functions.push(InvocationFunction {
        name: "double".to_string(),
        params: vec!["n".to_string()],
        body: InvocationProgram::new(vec![
            InvocationInstruction::LoadLocal {
                name: "n".to_string(),
            },
            konst(2),
            call("mul", 2),
            InvocationInstruction::Return,
        ]),
        max_depth: None,
    });
    let context = Value::Null;
    let catalog = catalog();
    let step = start(&module, &VmEnv::pure(&context, &catalog));
    assert_eq!(expect_value(step), Value::from(40i64));
}

#[test]
fn a_function_called_with_the_wrong_arity_is_rejected() {
    let mut module = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::Call {
            target: CallableTarget::Local {
                name: "one".to_string(),
            },
            argc: 0,
            names: Vec::new(),
            policy: None,
        },
        InvocationInstruction::Return,
    ]));
    module.functions.push(InvocationFunction {
        name: "one".to_string(),
        params: vec!["n".to_string()],
        body: InvocationProgram::new(vec![InvocationInstruction::Return]),
        max_depth: None,
    });
    let context = Value::Null;
    let catalog = catalog();
    assert!(matches!(
        start(&module, &VmEnv::pure(&context, &catalog)),
        InvocationStep::Failed { .. }
    ));
}

#[test]
fn unbounded_recursion_hits_the_frame_limit_instead_of_the_rust_stack() {
    let mut module = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::Call {
            target: CallableTarget::Local {
                name: "forever".to_string(),
            },
            argc: 0,
            names: Vec::new(),
            policy: None,
        },
        InvocationInstruction::Return,
    ]));
    module.functions.push(InvocationFunction {
        name: "forever".to_string(),
        params: Vec::new(),
        body: InvocationProgram::new(vec![
            InvocationInstruction::Call {
                target: CallableTarget::Local {
                    name: "forever".to_string(),
                },
                argc: 0,
                names: Vec::new(),
                policy: None,
            },
            InvocationInstruction::Return,
        ]),
        max_depth: None,
    });
    let context = Value::Null;
    let catalog = catalog();
    match start(&module, &VmEnv::pure(&context, &catalog)) {
        InvocationStep::Failed { message } => assert!(message.contains("recursion")),
        other => panic!("expected a recursion failure, got {other:?}"),
    }
}

#[test]
fn an_annotated_recursion_cap_is_enforced() {
    let mut module = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::Call {
            target: CallableTarget::Local {
                name: "deep".to_string(),
            },
            argc: 0,
            names: Vec::new(),
            policy: None,
        },
        InvocationInstruction::Return,
    ]));
    module.functions.push(InvocationFunction {
        name: "deep".to_string(),
        params: Vec::new(),
        body: InvocationProgram::new(vec![
            InvocationInstruction::Call {
                target: CallableTarget::Local {
                    name: "deep".to_string(),
                },
                argc: 0,
                names: Vec::new(),
                policy: None,
            },
            InvocationInstruction::Return,
        ]),
        max_depth: Some(3),
    });
    let context = Value::Null;
    let catalog = catalog();
    match start(&module, &VmEnv::pure(&context, &catalog)) {
        InvocationStep::Failed { message } => assert!(message.contains("recursion")),
        other => panic!("expected a recursion failure, got {other:?}"),
    }
}

#[test]
fn a_closure_captures_its_lexical_environment_and_applies() {
    // `let n = 10; (x => add(x, n))(5)` — n comes from where the closure was written.
    let value = expect_value(run_entry(vec![
        konst(10),
        InvocationInstruction::StoreLocal {
            name: "n".to_string(),
        },
        InvocationInstruction::Closure {
            params: vec!["x".to_string()],
            body: InvocationProgram::new(vec![
                InvocationInstruction::LoadLocal {
                    name: "x".to_string(),
                },
                InvocationInstruction::LoadLocal {
                    name: "n".to_string(),
                },
                call("add", 2),
                InvocationInstruction::Return,
            ]),
        },
        konst(5),
        InvocationInstruction::Apply { argc: 1 },
        InvocationInstruction::Return,
    ]));
    assert_eq!(value, Value::from(15i64));
}

#[test]
fn applying_a_non_closure_is_an_error() {
    let step = run_entry(vec![
        konst(1),
        konst(2),
        InvocationInstruction::Apply { argc: 1 },
    ]);
    assert!(matches!(step, InvocationStep::Failed { .. }));
}

#[test]
fn a_reference_resolves_against_the_run_context() {
    let mut input = Map::new();
    input.insert("name".to_string(), Value::from("ada"));
    let mut context = Map::new();
    context.insert("input".to_string(), Value::Object(input));
    let context = Value::Object(context);

    // a ref is `{ <root>: [<segment>, …] }`, so this is `input.name`.
    let mut reference = Map::new();
    reference.insert("input".to_string(), Value::Array(vec![Value::from("name")]));

    let module = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::LoadRef {
            reference: Value::Object(reference),
        },
        InvocationInstruction::Return,
    ]));
    let catalog = catalog();
    let step = start(&module, &VmEnv::pure(&context, &catalog));
    assert_eq!(expect_value(step), Value::from("ada"));
}

#[test]
fn evaluate_pure_returns_a_value_for_a_pure_program() {
    let catalog = catalog();
    let value = evaluate_pure(
        &InvocationProgram::new(vec![
            text("hi"),
            call("upper", 1),
            InvocationInstruction::Return,
        ]),
        &Value::Null,
        &catalog,
    )
    .expect("pure evaluation");
    assert_eq!(value, Value::from("HI"));
}

#[test]
fn evaluate_pure_refuses_a_program_that_would_dispatch() {
    let catalog = catalog();
    let err = evaluate_pure(
        &InvocationProgram::new(vec![
            provider_call("github", "deploy", 0),
            InvocationInstruction::Return,
        ]),
        &Value::Null,
        &catalog,
    )
    .expect_err("should refuse");
    assert!(err.to_string().contains("github.deploy"));
}

#[test]
fn evaluate_pure_refuses_a_goto() {
    let catalog = catalog();
    assert!(
        evaluate_pure(
            &InvocationProgram::new(vec![InvocationInstruction::Goto {
                target: "x".to_string()
            }]),
            &Value::Null,
            &catalog,
        )
        .is_err()
    );
}

#[test]
fn a_call_site_policy_travels_on_the_effect() {
    let policy = CallPolicy {
        timeout_seconds: Some(30),
        ..CallPolicy::default()
    };
    let module = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::Call {
            target: CallableTarget::Provider {
                provider: "svc".to_string(),
                function: "call".to_string(),
            },
            argc: 0,
            names: Vec::new(),
            policy: Some(policy),
        },
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    match start(&module, &VmEnv::pure(&context, &catalog)) {
        InvocationStep::Yield { effect, .. } => {
            assert_eq!(effect.policy.timeout_seconds, Some(30));
        }
        other => panic!("expected a yield, got {other:?}"),
    }
}

#[test]
fn successive_yields_carry_increasing_sequence_numbers() {
    // the sequence is what names a call for dedupe and attribution, so it must advance.
    let module = InvocationModule::new(InvocationProgram::new(vec![
        provider_call("svc", "one", 0),
        InvocationInstruction::Pop,
        provider_call("svc", "two", 0),
        InvocationInstruction::Return,
    ]));
    let context = Value::Null;
    let catalog = catalog();
    let env = VmEnv::pure(&context, &catalog);

    let InvocationStep::Yield {
        effect: first,
        continuation,
    } = start(&module, &env)
    else {
        panic!("expected a yield");
    };
    assert_eq!(first.sequence, 0);

    let InvocationStep::Yield { effect: second, .. } = resume(
        &module,
        *continuation,
        InvocationEffectResult::ok(Value::Null),
        &env,
    ) else {
        panic!("expected a second yield");
    };
    assert_eq!(second.sequence, 1);
    assert_eq!(second.target.display_name(), "svc.two");
}
