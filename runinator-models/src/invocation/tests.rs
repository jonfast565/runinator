//! covers the invocation ir's wire form: json round-trips, policy overlay, and the effect's
//! argument naming.

use super::*;
use crate::value::Map;

fn program() -> InvocationProgram {
    InvocationProgram::new(vec![
        InvocationInstruction::Const {
            value: Value::from(1i64),
        },
        InvocationInstruction::StoreLocal {
            name: "x".to_string(),
        },
        InvocationInstruction::LoadLocal {
            name: "x".to_string(),
        },
        InvocationInstruction::Return,
    ])
}

#[test]
fn module_round_trips_through_json() {
    let module = InvocationModule::new(program());
    let text = serde_json::to_string(&module).expect("serialize");
    let back: InvocationModule = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(module, back);
}

#[test]
fn a_new_module_is_stamped_with_the_current_version() {
    let module = InvocationModule::new(program());
    assert_eq!(module.version, INVOCATION_IR_VERSION);
    assert!(module.is_supported());
}

#[test]
fn a_module_from_another_ir_version_is_not_supported() {
    let mut module = InvocationModule::new(program());
    module.version = INVOCATION_IR_VERSION + 1;
    assert!(!module.is_supported());
}

#[test]
fn functions_are_looked_up_by_name() {
    let mut module = InvocationModule::new(program());
    module.functions.push(InvocationFunction {
        name: "double".to_string(),
        params: vec!["n".to_string()],
        body: program(),
        max_depth: Some(4),
    });
    assert!(module.function("double").is_some());
    assert!(module.function("missing").is_none());
}

#[test]
fn every_step_round_trips_through_json() {
    let steps = vec![
        InvocationStep::Complete {
            value: Value::from(7i64),
        },
        InvocationStep::Failed {
            message: "boom".to_string(),
        },
        InvocationStep::Goto {
            target: "done".to_string(),
        },
        InvocationStep::Yield {
            effect: Box::new(InvocationEffect {
                sequence: 0,
                target: CallableTarget::Provider {
                    provider: "github".to_string(),
                    function: "deploy".to_string(),
                },
                args: vec![Value::String("main".to_string())],
                names: vec!["branch".to_string()],
                policy: CallPolicy::default(),
            }),
            continuation: Box::new(InvocationContinuation::start()),
        },
    ];
    for step in steps {
        let text = serde_json::to_string(&step).expect("serialize");
        let back: InvocationStep = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(step, back);
    }
}

#[test]
fn only_a_yield_is_non_terminal() {
    assert!(InvocationStep::Complete { value: Value::Null }.is_terminal());
    assert!(
        InvocationStep::Goto {
            target: "x".to_string()
        }
        .is_terminal()
    );
    assert!(
        !InvocationStep::Yield {
            effect: Box::new(InvocationEffect {
                sequence: 0,
                target: CallableTarget::Intrinsic {
                    name: "now".to_string()
                },
                args: Vec::new(),
                names: Vec::new(),
                policy: CallPolicy::default(),
            }),
            continuation: Box::new(InvocationContinuation::start()),
        }
        .is_terminal()
    );
}

#[test]
fn effect_class_joins_to_the_stronger_class() {
    assert_eq!(
        EffectClass::Pure.join(EffectClass::Local),
        EffectClass::Local
    );
    assert_eq!(
        EffectClass::Local.join(EffectClass::Durable),
        EffectClass::Durable
    );
    assert_eq!(EffectClass::Pure.join(EffectClass::Pure), EffectClass::Pure);
}

#[test]
fn only_pure_and_local_run_in_process() {
    assert!(EffectClass::Pure.is_in_process());
    assert!(EffectClass::Local.is_in_process());
    assert!(!EffectClass::Durable.is_in_process());
    // unknown cannot be decided statically, so it is never treated as in-process.
    assert!(!EffectClass::Unknown.is_in_process());
}

#[test]
fn a_call_site_policy_overrides_the_node_default_field_by_field() {
    let base = CallPolicy {
        timeout_seconds: Some(60),
        runner: Some("default".to_string()),
        tags: vec!["a".to_string()],
        ..CallPolicy::default()
    };
    let site = CallPolicy {
        timeout_seconds: Some(30),
        ..CallPolicy::default()
    };
    let merged = site.overlay(&base);
    // the call site wins where it spoke.
    assert_eq!(merged.timeout_seconds, Some(30));
    // and inherits where it did not.
    assert_eq!(merged.runner.as_deref(), Some("default"));
    assert_eq!(merged.tags, vec!["a".to_string()]);
}

#[test]
fn an_empty_policy_inherits_everything() {
    let base = CallPolicy {
        timeout_seconds: Some(15),
        runner: Some("functions".to_string()),
        ..CallPolicy::default()
    };
    assert!(CallPolicy::default().is_empty());
    let merged = CallPolicy::default().overlay(&base);
    assert_eq!(merged.timeout_seconds, Some(15));
    assert_eq!(merged.runner.as_deref(), Some("functions"));
}

#[test]
fn named_arguments_bind_to_the_trailing_positions() {
    // two positional then two named: the names align to the *end* of the argument list.
    let effect = InvocationEffect {
        sequence: 3,
        target: CallableTarget::Intrinsic {
            name: "slice".to_string(),
        },
        args: vec![
            Value::from(1i64),
            Value::from(2i64),
            Value::from(3i64),
            Value::from(4i64),
        ],
        names: vec!["from".to_string(), "to".to_string()],
        policy: CallPolicy::default(),
    };
    let mut expected = Map::new();
    expected.insert("arg0".to_string(), Value::from(1i64));
    expected.insert("arg1".to_string(), Value::from(2i64));
    expected.insert("from".to_string(), Value::from(3i64));
    expected.insert("to".to_string(), Value::from(4i64));
    assert_eq!(effect.to_parameters(), Value::Object(expected));
}

#[test]
fn a_fresh_continuation_starts_in_the_entry_frame() {
    let continuation = InvocationContinuation::start();
    let frame = continuation.current().expect("a frame");
    assert_eq!(frame.ip, 0);
    assert!(frame.function.is_none());
    assert!(!frame.awaiting);
    assert_eq!(continuation.call_sequence, 0);
}

#[test]
fn a_continuation_round_trips_with_its_recorded_locals() {
    let mut continuation = InvocationContinuation::start();
    continuation.call_sequence = 2;
    continuation.recorded.push(RecordedLocal {
        sequence: 1,
        name: "now".to_string(),
        value: Value::String("2026-08-15T00:00:00Z".to_string()),
    });
    continuation.frames.push(InvocationFrame::for_function(
        "helper",
        vec![("n".to_string(), Value::from(3i64))],
    ));
    let text = serde_json::to_string(&continuation).expect("serialize");
    let back: InvocationContinuation = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(continuation, back);
}

#[test]
fn a_packaged_target_reads_its_pinning_off_the_binding() {
    let binding = crate::functions::FunctionBinding {
        package_id: uuid::Uuid::nil(),
        package_name: "image_tools".to_string(),
        namespace: None,
        version_id: uuid::Uuid::nil(),
        version: 4,
        export_id: uuid::Uuid::nil(),
        export_name: "resize".to_string(),
        artifact_digest: "sha256:abc".to_string(),
    };
    let target = CallableTarget::Packaged {
        binding: binding.clone(),
    };
    assert_eq!(target.binding().expect("binding").version, 4);
    assert!(target.display_name().ends_with("resize"));
}

#[test]
fn target_display_names_read_the_way_an_author_wrote_them() {
    assert_eq!(
        CallableTarget::Provider {
            provider: "slack".to_string(),
            function: "post".to_string(),
        }
        .display_name(),
        "slack.post"
    );
    assert_eq!(
        CallableTarget::Intrinsic {
            name: "upper".to_string()
        }
        .display_name(),
        "upper"
    );
}
