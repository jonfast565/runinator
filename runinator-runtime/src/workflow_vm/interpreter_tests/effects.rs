//! effects interpreter regressions.

use super::*;

#[test]
fn freezes_action_references_before_yielding() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Effect {
        request: WorkflowEffectRequest::Action {
            provider: "test".into(),
            function: "run".into(),
            input: runinator_models::json!({
                "customer": { "$ref": { "input": ["customer"] } }
            }),
            timeout_seconds: Some(10),
            retry: Default::default(),
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: Some(runinator_models::json!({
                "$ref": { "input": ["request_id"] }
            })),
            function_binding: None,
        },
    }]);
    let mut continuation = continuation();
    continuation.locals.insert(
        "input".into(),
        runinator_models::json!({
            "customer": "acme",
            "request_id": "request-7"
        }),
    );

    let result = step(&module, continuation);
    let WorkflowVmStep::Yield { request, .. } = result else {
        panic!("action must yield, got {result:?}");
    };
    let WorkflowEffectRequest::Action {
        input,
        idempotency_key,
        ..
    } = *request
    else {
        panic!("expected action request");
    };
    assert_eq!(input, runinator_models::json!({ "customer": "acme" }));
    assert_eq!(idempotency_key, Some(Value::String("request-7".into())));
}

#[test]
fn foreign_code_effect_freezes_default_runtime_object_and_context() {
    let request = WorkflowEffectRequest::Action {
        provider: "std".into(),
        function: "code".into(),
        input: runinator_models::json!({
            "language": "gnucobol",
            "source": "identification division."
        }),
        timeout_seconds: Some(30),
        retry: Default::default(),
        tags: Vec::new(),
        required_labels: Default::default(),
        workspace_affinity: None,
        execution_profile: None,
        idempotency_key: None,
        function_binding: None,
    };
    let mut continuation = continuation();
    continuation
        .locals
        .insert("input".into(), runinator_models::json!({ "value": 41 }));
    continuation
        .locals
        .insert("config".into(), runinator_models::json!({}));

    let WorkflowEffectRequest::Action { input, .. } =
        resolve_effect_request(request, &continuation).unwrap()
    else {
        panic!("expected action request");
    };
    assert_eq!(input["language"], "cobol");
    assert_eq!(input["runtime"]["image"], "debian:bookworm-slim");
    assert!(
        input["runtime"]["setup_script"]
            .as_str()
            .unwrap()
            .contains("gnucobol")
    );
    assert_eq!(input["runtime"]["toolchain"]["executable"], "cobc");
    assert_eq!(input["runtime"]["limits"]["memory_mb"], 2048);
    assert_eq!(input["runtime"]["limits"]["cpu_millis"], 2000);
    assert_eq!(input["runtime"]["limits"]["pids"], 256);
    assert_eq!(input["runtime"]["limits"]["tmpfs_mb"], 512);
    assert_eq!(input["runtime"]["limits"]["max_output_bytes"], 1048576);
    assert_eq!(input["context"]["input"]["value"], 41);
}

#[test]
fn foreign_code_effect_uses_snapshotted_runtime_override() {
    let request = WorkflowEffectRequest::Action {
        provider: "std".into(),
        function: "code".into(),
        input: runinator_models::json!({
            "language": "cl",
            "source": "(defun main (context) context)"
        }),
        timeout_seconds: Some(30),
        retry: Default::default(),
        tags: Vec::new(),
        required_labels: Default::default(),
        workspace_affinity: None,
        execution_profile: None,
        idempotency_key: None,
        function_binding: None,
    };
    let mut continuation = continuation();
    continuation.locals.insert(
        "config".into(),
        runinator_models::json!({
            "foreign_languages": {
                "commonlisp": {
                    "image": "registry.example/runinator-sbcl:stable",
                    "setup_script": "",
                    "environment": { "LANG": "C.UTF-8" },
                    "toolchain": { "run_args": ["--dynamic-space-size", "2048"] },
                    "limits": { "memory_mb": 4096 }
                }
            }
        }),
    );

    let WorkflowEffectRequest::Action { input, .. } =
        resolve_effect_request(request, &continuation).unwrap()
    else {
        panic!("expected action request");
    };
    assert_eq!(input["language"], "commonlisp");
    assert_eq!(
        input["runtime"]["image"],
        "registry.example/runinator-sbcl:stable"
    );
    assert_eq!(input["runtime"]["setup_script"], "");
    assert_eq!(input["runtime"]["toolchain"]["executable"], "sbcl");
    assert_eq!(
        input["runtime"]["toolchain"]["run_args"][0],
        "--dynamic-space-size"
    );
    assert_eq!(input["runtime"]["environment"]["LANG"], "C.UTF-8");
    assert_eq!(input["runtime"]["limits"]["memory_mb"], 4096);
    assert_eq!(input["runtime"]["limits"]["cpu_millis"], 2000);
}

#[test]
fn freezes_loop_and_map_locals_in_action_inputs() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Effect {
        request: WorkflowEffectRequest::Action {
            provider: "test".into(),
            function: "run".into(),
            input: runinator_models::json!({
                "item": { "$ref": { "let": ["map_7.item"] } },
                "rendered": { "$to_string": { "$ref": { "let": ["map_7.item"] } } }
            }),
            timeout_seconds: Some(10),
            retry: Default::default(),
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        },
    }]);
    let mut continuation = continuation();
    continuation
        .locals
        .insert("map_7.item".into(), Value::String("alpha".into()));

    let result = step(&module, continuation);
    let WorkflowVmStep::Yield { request, .. } = result else {
        panic!("action must yield, got {result:?}");
    };
    let WorkflowEffectRequest::Action { input, .. } = *request else {
        panic!("expected action request");
    };
    assert_eq!(
        input,
        runinator_models::json!({ "item": "alpha", "rendered": "alpha" })
    );
}

#[test]
fn yields_once_and_resumes_with_the_effect_value() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Timer { due_at: 1 },
        },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        ..
    } = step(&module, continuation())
    else {
        panic!("expected effect yield");
    };
    assert_eq!(sequence, 0);
    assert_eq!(continuation.awaiting_effect_id, Some(effect_id));
    assert_eq!(continuation.status, WorkflowContinuationStatus::Waiting);
    let WorkflowVmStep::Complete { value, .. } = resume(
        &module,
        continuation,
        None,
        Ok(Value::String("done".into())),
    ) else {
        panic!("expected completion");
    };
    assert_eq!(value, Value::String("done".into()));
}

#[test]
fn refuses_a_duplicate_resume_after_the_wait_was_consumed() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::Timer { due_at: 1 },
        },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Yield { continuation, .. } = step(&module, continuation()) else {
        panic!("expected effect yield");
    };
    let WorkflowVmStep::Complete { continuation, .. } =
        resume(&module, continuation, None, Ok(Value::Null))
    else {
        panic!("expected completion");
    };
    assert!(matches!(
        resume(&module, continuation, None, Ok(Value::Null)),
        WorkflowVmStep::Failed { .. }
    ));
}

#[test]
fn duplicate_drive_yields_the_same_logical_effect() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Effect {
        request: WorkflowEffectRequest::Timer { due_at: 1 },
    }]);
    let continuation = continuation();
    let WorkflowVmStep::Yield {
        effect_id: first_id,
        sequence: first_sequence,
        ..
    } = step(&module, continuation.clone())
    else {
        panic!("expected yield");
    };
    let WorkflowVmStep::Yield {
        effect_id: second_id,
        sequence: second_sequence,
        ..
    } = step(&module, continuation)
    else {
        panic!("expected yield");
    };
    assert_eq!((first_id, first_sequence), (second_id, second_sequence));
}

#[test]
fn parking_effects_yield_resume_and_restart_stably() {
    // Each parking request has exactly the same host-free lifecycle: stepping yields one
    // durable effect, replaying the unmodified continuation yields that same receipt, and a
    // settled typed value resumes into the next instruction. The coordination host never
    // needs a node-specific polling loop to make progress.
    let requests = vec![
        WorkflowEffectRequest::TimerDelay { seconds: 30 },
        WorkflowEffectRequest::Approval {
            prompt: Value::String("approve deployment".into()),
            expires_at: None,
        },
        WorkflowEffectRequest::Gate {
            kind: GateKind::Manual,
            condition: Default::default(),
            poll_interval_seconds: 30,
            deadline_seconds: Some(300),
            continue_on_timeout: false,
            label: Some("production".into()),
            metadata: Value::Null,
        },
        WorkflowEffectRequest::Signal {
            key: "release-ready".into(),
            filter: Some(Value::String("release-42".into())),
        },
        WorkflowEffectRequest::Input {
            prompt: Some("version".into()),
            schema: Value::Null,
        },
        WorkflowEffectRequest::EventWait {
            event_type: "build.finished".into(),
            filter: None,
            max_events: Some(1),
        },
        WorkflowEffectRequest::ChildRun {
            workflow_id: Some(Uuid::nil()),
            workflow_name: None,
            workflow_revision: None,
            workflow_revision_digest: None,
            input: Value::Null,
            wait: true,
            reuse_open_run: false,
            run_name: None,
        },
        WorkflowEffectRequest::AwaitRun {
            workflow: "child".into(),
            key: None,
            run_id: None,
            mode: "all".into(),
        },
    ];

    for request in requests {
        let module = WorkflowModule::new(vec![
            WorkflowInstruction::Effect {
                request: request.clone(),
            },
            WorkflowInstruction::Return,
        ]);
        let start = continuation();
        let WorkflowVmStep::Yield {
            continuation: waiting,
            effect_id,
            sequence,
            request: yielded,
        } = step(&module, start.clone())
        else {
            panic!("parking request must yield");
        };
        assert_eq!(*yielded, request);

        let restarted: WorkflowContinuation =
            serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
        let WorkflowVmStep::Yield {
            effect_id: replayed_id,
            sequence: replayed_sequence,
            request: replayed_request,
            ..
        } = step(&module, restarted)
        else {
            panic!("restarted parking request must yield");
        };
        assert_eq!(
            (replayed_id, replayed_sequence, replayed_request),
            (effect_id, sequence, Box::new(request))
        );

        let value = Value::String("settled".into());
        let WorkflowVmStep::Complete {
            value: completed, ..
        } = resume(&module, waiting, None, Ok(value.clone()))
        else {
            panic!("settled parking request must resume");
        };
        assert_eq!(completed, value);
    }
}
