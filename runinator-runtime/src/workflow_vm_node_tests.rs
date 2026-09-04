//! Compiled-graph VM coverage for every author-facing workflow node kind.
//!
//! The unit tests beside the interpreter exercise individual opcodes.  These tests deliberately
//! compile ordinary workflow graphs first, then drive the resulting continuations.  That pins the
//! compiler/VM contract at the places where node data crosses a durable boundary.

use std::collections::BTreeMap;

use runinator_models::{
    interrupt::InterruptSource,
    invocation::{InvocationInstruction, InvocationModule, InvocationProgram},
    orchestration::GateKind,
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest, WorkflowFailure,
        WorkflowFailureKind, WorkflowInterruptOutcome, WorkflowPendingInterrupt,
    },
    workflows::{
        WorkflowAction, WorkflowBranch, WorkflowCondition, WorkflowDefinition, WorkflowGraph,
        WorkflowNode, WorkflowNodeKind, WorkflowNodeRef, WorkflowSubflowType, WorkflowTransitions,
    },
};
use runinator_workflows::compile_workflow_module;
use uuid::Uuid;

use crate::{WorkflowVmStep, resume_workflow_vm, step_workflow_vm};

fn params(value: Value) -> runinator_models::workflows::WorkflowObject {
    runinator_models::workflows::WorkflowObject::from_value(value)
        .expect("node parameters must be an object")
}

fn node(id: &str, kind: WorkflowNodeKind, next: Option<&str>) -> WorkflowNode {
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

fn action(id: &str, next: Option<&str>, input: Value) -> WorkflowNode {
    let mut node = node(id, WorkflowNodeKind::Action, next);
    node.action = Some(WorkflowAction {
        provider: "demo".into(),
        function: id.into(),
        timeout_seconds: 30,
        configuration: params(input),
        mcp_enabled: false,
        tags: vec!["vm-node-suite".into()],
        required_labels: BTreeMap::new(),
        workspace_affinity: None,
        execution_profile: None,
        idempotency_key: Some(runinator_models::json!({
            "$ref": { "input": ["request_id"] }
        })),
        function_binding: None,
    });
    node
}

fn definition(name: &str, nodes: Vec<WorkflowNode>, metadata: Value) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.into(),
        key: None,
        namespace: None,
        org_id: None,
        version: Default::default(),
        enabled: true,
        input_type: Default::default(),
        definition: WorkflowGraph {
            start: Some("start".into()),
            nodes,
            metadata,
            ..Default::default()
        },
        created_at: None,
        updated_at: None,
    }
}

fn compile(name: &str, nodes: Vec<WorkflowNode>) -> runinator_models::workflow_vm::WorkflowModule {
    compile_with_metadata(name, nodes, Value::Null)
}

fn compile_with_metadata(
    name: &str,
    nodes: Vec<WorkflowNode>,
    metadata: Value,
) -> runinator_models::workflow_vm::WorkflowModule {
    compile_workflow_module(&definition(name, nodes, metadata))
        .unwrap_or_else(|error| panic!("{name} must compile: {error}"))
}

fn continuation(module: &runinator_models::workflow_vm::WorkflowModule) -> WorkflowContinuation {
    WorkflowContinuation::start(Uuid::now_v7(), module.version)
}

fn complete(
    module: &runinator_models::workflow_vm::WorkflowModule,
    continuation: WorkflowContinuation,
) -> (Value, WorkflowContinuation) {
    match step_workflow_vm(module, continuation) {
        WorkflowVmStep::Complete {
            value,
            continuation,
        } => (value, continuation),
        other => panic!("expected the compiled graph to complete, got {other:?}"),
    }
}

fn output(id: &str, data: Value) -> WorkflowNode {
    let mut node = node(id, WorkflowNodeKind::Output, Some("end"));
    node.parameters = params(runinator_models::json!({ "data": data }));
    node
}

#[test]
fn compiled_linear_nodes_freeze_effect_data_and_preserve_settlements() {
    // This is intentionally one long graph: every durable node resumes into the next one.  The
    // stack assertions therefore prove that a settlement survives the next node's boundary, not
    // merely that each request can be yielded in isolation.
    let mut config = node("config", WorkflowNodeKind::Config, Some("transform"));
    config.parameters = params(runinator_models::json!({
        "name": { "$ref": { "input": ["release"] } },
        "metadata": { "request": { "$ref": { "input": ["request_id"] } } }
    }));

    let mut transform = node("transform", WorkflowNodeKind::Transform, Some("assert"));
    transform.parameters = params(runinator_models::json!({
        "bindings": { "request": { "$ref": { "input": ["request_id"] } } }
    }));

    let mut assertion = node("assert", WorkflowNodeKind::Assert, Some("invoke"));
    assertion.parameters = params(runinator_models::json!({ "assertions": [] }));

    let mut invoke = node("invoke", WorkflowNodeKind::Invocation, Some("action"));
    let invocation = InvocationModule::new(InvocationProgram::new(vec![
        InvocationInstruction::Const {
            value: Value::String("invoked".into()),
        },
        InvocationInstruction::Return,
    ]));
    invoke.parameters = params(serde_json::json!({ "module": invocation }).into());

    let mut wait = node("wait", WorkflowNodeKind::Wait, Some("approval"));
    wait.wait.seconds = Some(runinator_models::workflows::WorkflowWaitSeconds::Integer(5));

    let mut approval = node("approval", WorkflowNodeKind::Approval, Some("gate"));
    approval.parameters = params(runinator_models::json!({
        "approval_type": "release",
        "prompt": "Ship this build?"
    }));

    let mut gate = node("gate", WorkflowNodeKind::Gate, Some("signal"));
    gate.parameters = params(runinator_models::json!({
        "kind": "manual",
        "poll_interval": 15,
        "timeout": 90,
        "timeout_policy": "continue",
        "label": "production"
    }));

    let mut signal = node("signal", WorkflowNodeKind::Signal, Some("input"));
    signal.parameters = params(runinator_models::json!({
        "name": "release-ready",
        "correlation_key": { "$ref": { "input": ["request_id"] } }
    }));

    let mut input = node("input", WorkflowNodeKind::Input, Some("subflow"));
    input.parameters = params(runinator_models::json!({
        "prompt": "Version",
        "default": { "$ref": { "input": ["release"] } }
    }));

    let mut subflow = node("subflow", WorkflowNodeKind::Subflow, Some("audit"));
    subflow.subflow_id = Some(Uuid::nil());
    subflow.subflow.subflow_type = WorkflowSubflowType::Wait;
    subflow.parameters = params(runinator_models::json!({
        "release": { "$ref": { "input": ["release"] } }
    }));

    let coordination = [
        ("audit", WorkflowNodeKind::Audit, "checkpoint"),
        ("checkpoint", WorkflowNodeKind::Checkpoint, "mutex"),
        ("mutex", WorkflowNodeKind::Mutex, "throttle"),
        ("throttle", WorkflowNodeKind::Throttle, "cooldown"),
        ("cooldown", WorkflowNodeKind::Cooldown, "await"),
        ("await", WorkflowNodeKind::AwaitRun, "debounce"),
        ("debounce", WorkflowNodeKind::Debounce, "collect"),
        ("collect", WorkflowNodeKind::Collect, "barrier"),
        ("barrier", WorkflowNodeKind::Barrier, "circuit"),
        ("circuit", WorkflowNodeKind::CircuitBreaker, "event"),
    ];
    let mut coordination_nodes = coordination
        .into_iter()
        .map(|(id, kind, next)| {
            let mut node = node(id, kind, Some(next));
            node.parameters = params(runinator_models::json!({
                "name": id,
                "request": { "$ref": { "input": ["request_id"] } }
            }));
            node
        })
        .collect::<Vec<_>>();
    let await_node = coordination_nodes
        .iter_mut()
        .find(|node| node.id == "await")
        .expect("await node is part of the chain");
    await_node.parameters = params(runinator_models::json!({
        "workflow": "child",
        "key": { "$ref": { "input": ["request_id"] } },
        "mode": "any"
    }));

    let mut event = node("event", WorkflowNodeKind::EventSource, Some("output"));
    event.parameters = params(runinator_models::json!({
        "event_type": "build.finished",
        "filter": { "request": { "$ref": { "input": ["request_id"] } } },
        "max": 1
    }));

    let mut output = node("output", WorkflowNodeKind::Output, Some("end"));
    output.parameters = params(runinator_models::json!({
        "event_type": "release.completed",
        "data": { "request": { "$ref": { "input": ["request_id"] } } },
        "items": [{
            "name": "receipt",
            "source": { "$ref": { "input": ["receipt"] } }
        }]
    }));

    let mut nodes = vec![
        node("start", WorkflowNodeKind::Start, Some("config")),
        config,
        transform,
        assertion,
        invoke,
        action(
            "action",
            Some("wait"),
            runinator_models::json!({
                "request": { "$ref": { "input": ["request_id"] } }
            }),
        ),
        wait,
        approval,
        gate,
        signal,
        input,
        subflow,
    ];
    nodes.append(&mut coordination_nodes);
    nodes.extend([event, output, node("end", WorkflowNodeKind::End, None)]);
    let module = compile("linear-node-suite", nodes);

    let mut current = continuation(&module);
    current.locals.insert(
        "input".into(),
        runinator_models::json!({
            "request_id": "request-7",
            "release": "2026.08.23",
            "receipt": "receipt-7"
        }),
    );

    let expected_nodes = [
        "action",
        "wait",
        "approval",
        "gate",
        "signal",
        "input",
        "subflow",
        "audit",
        "checkpoint",
        "mutex",
        "throttle",
        "cooldown",
        "await",
        "debounce",
        "collect",
        "barrier",
        "circuit",
        "event",
    ];
    let mut settled = Vec::with_capacity(expected_nodes.len());
    let mut previous_request = None;

    for (sequence, node_id) in expected_nodes.iter().enumerate() {
        let step = match previous_request.as_ref() {
            None => step_workflow_vm(&module, current),
            Some(request) => {
                let value = Value::String(format!("settled-{}", sequence - 1));
                settled.push(value.clone());
                resume_workflow_vm(&module, current, Some(request), Ok(value))
            }
        };
        let WorkflowVmStep::Yield {
            continuation,
            sequence: yielded_sequence,
            request,
            ..
        } = step
        else {
            panic!("{node_id} must yield exactly one durable request");
        };
        assert_eq!(yielded_sequence, sequence as u64);
        assert_eq!(
            continuation.pending_node_entries.last(),
            Some(&node_id.to_string())
        );
        if let Some(previous) = settled.last() {
            assert_eq!(continuation.stack.last(), Some(previous));
        }
        assert_linear_request(node_id, &request);
        previous_request = Some(*request);
        current = continuation;
    }

    let request = previous_request.expect("the event node yielded");
    let final_settlement = Value::String(format!("settled-{}", expected_nodes.len() - 1));
    settled.push(final_settlement.clone());
    let WorkflowVmStep::Complete {
        value,
        continuation,
    } = resume_workflow_vm(&module, current, Some(&request), Ok(final_settlement))
    else {
        panic!("output and end must complete after the final event");
    };
    assert_eq!(value, runinator_models::json!({ "request": "request-7" }));
    assert!(
        settled
            .iter()
            .all(|value| continuation.stack.contains(value)),
        "every settled value must remain available after traversing the rest of the chain"
    );
    assert_eq!(
        continuation.locals["__workflow_vm_output"]["artifacts"]["receipt"],
        Value::String("receipt-7".into())
    );
}

fn assert_linear_request(node_id: &str, request: &WorkflowEffectRequest) {
    match (node_id, request) {
        (
            "action",
            WorkflowEffectRequest::Action {
                provider,
                function,
                input,
                idempotency_key,
                ..
            },
        ) => {
            assert_eq!(provider, "demo");
            assert_eq!(function, "action");
            assert_eq!(input, &runinator_models::json!({ "request": "request-7" }));
            assert_eq!(idempotency_key, &Some(Value::String("request-7".into())));
        }
        ("wait", WorkflowEffectRequest::TimerDelay { seconds }) => assert_eq!(*seconds, 5),
        ("approval", WorkflowEffectRequest::Approval { prompt, .. }) => {
            assert_eq!(prompt["approval_type"], Value::String("release".into()));
            assert_eq!(prompt["prompt"], Value::String("Ship this build?".into()));
        }
        (
            "gate",
            WorkflowEffectRequest::Gate {
                kind,
                poll_interval_seconds,
                deadline_seconds,
                continue_on_timeout,
                label,
                ..
            },
        ) => {
            assert_eq!(*kind, GateKind::Manual);
            assert_eq!(*poll_interval_seconds, 15);
            assert_eq!(*deadline_seconds, Some(90));
            assert!(*continue_on_timeout);
            assert_eq!(label.as_deref(), Some("production"));
        }
        ("signal", WorkflowEffectRequest::Signal { key, filter }) => {
            assert_eq!(key, "release-ready");
            assert_eq!(filter, &Some(Value::String("request-7".into())));
        }
        ("input", WorkflowEffectRequest::Input { prompt, schema }) => {
            assert_eq!(prompt.as_deref(), Some("Version"));
            assert_eq!(schema["default"], Value::String("2026.08.23".into()));
        }
        (
            "subflow",
            WorkflowEffectRequest::ChildRun {
                workflow_id,
                input,
                wait,
                ..
            },
        ) => {
            assert_eq!(*workflow_id, Some(Uuid::nil()));
            assert!(*wait);
            assert_eq!(input, &runinator_models::json!({ "release": "2026.08.23" }));
        }
        (
            "await",
            WorkflowEffectRequest::AwaitRun {
                workflow,
                key,
                mode,
                ..
            },
        ) => {
            assert_eq!(workflow, "child");
            assert_eq!(key, &Some(Value::String("request-7".into())));
            assert_eq!(mode, "any");
        }
        (
            "event",
            WorkflowEffectRequest::EventWait {
                event_type,
                filter,
                max_events,
            },
        ) => {
            assert_eq!(event_type, "build.finished");
            assert_eq!(
                filter,
                &Some(runinator_models::json!({ "request": "request-7" }))
            );
            assert_eq!(*max_events, Some(1));
        }
        (
            node_id @ ("audit" | "checkpoint" | "mutex" | "throttle" | "cooldown" | "debounce"
            | "collect" | "barrier" | "circuit"),
            WorkflowEffectRequest::Coordination { kind, input },
        ) => {
            let expected_kind = if node_id == "circuit" {
                "circuit_breaker"
            } else {
                node_id
            };
            assert_eq!(kind, expected_kind);
            assert_eq!(input["request"], Value::String("request-7".into()));
        }
        (node_id, request) => panic!("{node_id} yielded the wrong request: {request:?}"),
    }
}

#[test]
fn compiled_routing_nodes_take_the_expected_data_path() {
    let mut condition = node("condition", WorkflowNodeKind::Condition, Some("no"));
    condition.transitions.branches = vec![WorkflowBranch {
        when: WorkflowCondition::from_value(runinator_models::json!({
            "value": "release",
            "equals": "release"
        })),
        target: WorkflowNodeRef::new("yes"),
        priority: None,
    }];
    let module = compile(
        "condition-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("condition")),
            condition,
            output("yes", Value::String("condition".into())),
            output("no", Value::String("unexpected".into())),
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    assert_eq!(
        complete(&module, continuation(&module)).0,
        Value::String("condition".into())
    );

    let mut switch = node("switch", WorkflowNodeKind::Switch, None);
    switch.parameters = params(runinator_models::json!({
        "value": "blue",
        "cases": [{ "equals": "blue", "target": { "$node": "yes" } }],
        "default": { "$node": "no" }
    }));
    let module = compile(
        "switch-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("switch")),
            switch,
            output("yes", Value::String("switch".into())),
            output("no", Value::String("unexpected".into())),
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    assert_eq!(
        complete(&module, continuation(&module)).0,
        Value::String("switch".into())
    );

    let mut toggle = node("toggle", WorkflowNodeKind::Toggle, None);
    toggle.parameters = params(runinator_models::json!({
        "value": true,
        "on": { "$node": "yes" },
        "off": { "$node": "no" }
    }));
    let module = compile(
        "toggle-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("toggle")),
            toggle,
            output("yes", Value::String("toggle".into())),
            output("no", Value::String("unexpected".into())),
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    assert_eq!(
        complete(&module, continuation(&module)).0,
        Value::String("toggle".into())
    );

    let mut percentage = node("percentage", WorkflowNodeKind::Percentage, None);
    percentage.parameters = params(runinator_models::json!({
        "key": null,
        "buckets": [{ "weight": 1, "target": { "$node": "yes" } }],
        "default": { "$node": "no" }
    }));
    let module = compile(
        "percentage-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("percentage")),
            percentage,
            output("yes", Value::String("percentage".into())),
            output("no", Value::String("percentage-default".into())),
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    assert_eq!(
        complete(&module, continuation(&module)).0,
        Value::String("percentage-default".into())
    );
}

#[test]
fn compiled_loop_and_try_nodes_keep_body_data_and_route_failures() {
    let mut loop_node = node("loop", WorkflowNodeKind::Loop, Some("body"));
    loop_node.parameters = params(runinator_models::json!({ "items": [1, 2] }));
    loop_node.transitions.on_success = Some(WorkflowNodeRef::new("end"));
    let mut body = node("body", WorkflowNodeKind::Config, Some("loop"));
    body.parameters = params(runinator_models::json!({ "value": "body" }));
    let module = compile(
        "loop-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("loop")),
            loop_node,
            body,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    assert_eq!(
        complete(&module, continuation(&module)).0,
        runinator_models::json!([{ "value": "body" }, { "value": "body" }])
    );

    let mut guard = node("guard", WorkflowNodeKind::Try, Some("end"));
    guard.parameters = params(runinator_models::json!({
        "body": { "$node": "body" },
        "catch": { "$node": "catch" },
        "finally": { "$node": "finally" }
    }));
    let mut catch = node("catch", WorkflowNodeKind::Config, Some("guard"));
    catch.parameters = params(runinator_models::json!({ "caught": true }));
    let mut finally = node("finally", WorkflowNodeKind::Transform, Some("guard"));
    finally.parameters = params(runinator_models::json!({ "finally": true }));
    let module = compile(
        "try-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("guard")),
            guard,
            action("body", Some("guard"), runinator_models::json!({})),
            catch,
            finally,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    let mut try_continuation = continuation(&module);
    try_continuation.locals.insert(
        "input".into(),
        runinator_models::json!({ "request_id": "try-7" }),
    );
    let WorkflowVmStep::Yield {
        continuation,
        request,
        ..
    } = step_workflow_vm(&module, try_continuation)
    else {
        panic!("the try body action must yield");
    };
    let WorkflowVmStep::Complete { value, .. } = resume_workflow_vm(
        &module,
        continuation,
        Some(&request),
        Err(WorkflowFailure::new(
            WorkflowFailureKind::Failed,
            "body failed",
        )),
    ) else {
        panic!("a failed try body must drive through catch and finally");
    };
    assert_eq!(value, runinator_models::json!({ "finally": true }));
}

#[test]
fn compiled_fan_out_nodes_bind_branch_data_before_the_host_merges_it() {
    let mut parallel = node("parallel", WorkflowNodeKind::Parallel, None);
    parallel.parameters = params(runinator_models::json!({
        "branches": [{ "$node": "left" }, { "$node": "right" }]
    }));
    let mut left = node("left", WorkflowNodeKind::Config, Some("join"));
    left.parameters = params(runinator_models::json!({ "branch": "left" }));
    let mut right = node("right", WorkflowNodeKind::Config, Some("join"));
    right.parameters = params(runinator_models::json!({ "branch": "right" }));
    let mut join = node("join", WorkflowNodeKind::Join, Some("end"));
    join.parameters = params(runinator_models::json!({
        "wait_for": [{ "$node": "left" }, { "$node": "right" }],
        "mode": "all"
    }));
    let module = compile(
        "parallel-and-join-nodes",
        vec![
            node("start", WorkflowNodeKind::Start, Some("parallel")),
            parallel,
            left,
            right,
            join,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    let WorkflowVmStep::Fork {
        parent, children, ..
    } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("parallel must fork its two branches");
    };
    assert_eq!(parent.status, WorkflowContinuationStatus::Joined);
    assert_eq!(children.len(), 2);
    let branch_values = children
        .into_iter()
        .map(|child| match step_workflow_vm(&module, child) {
            WorkflowVmStep::Joined {
                join_key, value, ..
            } => {
                assert_eq!(join_key, "join");
                value
            }
            other => panic!("parallel branch must arrive at join, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        branch_values,
        vec![
            runinator_models::json!({ "branch": "left" }),
            runinator_models::json!({ "branch": "right" })
        ]
    );

    let mut map = node("map", WorkflowNodeKind::Map, Some("end"));
    map.parameters = params(runinator_models::json!({
        "items": ["a", "b"],
        "target": { "$node": "work" },
        "concurrency": 2
    }));
    let mut work = node("work", WorkflowNodeKind::Config, Some("map"));
    work.parameters = params(runinator_models::json!({ "mapped": true }));
    let module = compile(
        "map-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("map")),
            map,
            work,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    let WorkflowVmStep::Fork { children, .. } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("map must fork its concurrency window");
    };
    let map_results = children
        .into_iter()
        .map(|child| {
            let item = child.locals["map.item"].clone();
            match step_workflow_vm(&module, child) {
                WorkflowVmStep::Joined {
                    join_key, value, ..
                } => {
                    assert_eq!(join_key, "map");
                    (item, value)
                }
                other => panic!("map item must return its body result, got {other:?}"),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        map_results,
        vec![
            (
                Value::String("a".into()),
                runinator_models::json!({ "mapped": true })
            ),
            (
                Value::String("b".into()),
                runinator_models::json!({ "mapped": true })
            )
        ]
    );

    let mut race = node("race", WorkflowNodeKind::Race, None);
    race.parameters = params(runinator_models::json!({
        "branches": [{ "$node": "fast" }, { "$node": "slow" }],
        "winner": "first_success"
    }));
    let mut fast = node("fast", WorkflowNodeKind::Config, Some("end"));
    fast.parameters = params(runinator_models::json!({ "winner": "fast" }));
    let mut slow = node("slow", WorkflowNodeKind::Config, Some("end"));
    slow.parameters = params(runinator_models::json!({ "winner": "slow" }));
    let module = compile(
        "race-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("race")),
            race,
            fast,
            slow,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    let WorkflowVmStep::Fork {
        parent, children, ..
    } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("race must fork its contenders");
    };
    assert!(parent.frames.iter().any(|frame| matches!(
        frame,
        runinator_models::workflow_vm::WorkflowFrame::Race(frame)
            if frame.winner_policy == runinator_models::workflow_vm::WorkflowBranchPolicy::FirstSuccess
    )));
    let contender_values = children
        .into_iter()
        .map(|child| match step_workflow_vm(&module, child) {
            WorkflowVmStep::Complete { value, .. } => value,
            other => panic!("race contender must preserve its value, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        contender_values,
        vec![
            runinator_models::json!({ "winner": "fast" }),
            runinator_models::json!({ "winner": "slow" })
        ]
    );
}

#[test]
fn compiled_terminal_and_interrupt_nodes_return_control_to_the_main_flow() {
    let mut fail = node("fail", WorkflowNodeKind::Fail, None);
    fail.parameters = params(runinator_models::json!({ "message": "expected failure" }));
    let module = compile(
        "fail-node",
        vec![
            node("start", WorkflowNodeKind::Start, Some("fail")),
            fail,
            node("end", WorkflowNodeKind::End, None),
        ],
    );
    let WorkflowVmStep::Failed { message, .. } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("the fail node must fail the continuation");
    };
    assert_eq!(message, "expected failure");

    let handler = node("handler", WorkflowNodeKind::Interrupt, Some("handler_note"));
    let mut handler_note = node("handler_note", WorkflowNodeKind::Config, Some("resume"));
    handler_note.parameters = params(runinator_models::json!({ "handled": true }));
    let mut resume = node("resume", WorkflowNodeKind::Resume, None);
    resume.parameters = params(runinator_models::json!({ "mode": "resume" }));
    let mut wait = node("wait", WorkflowNodeKind::Wait, Some("end"));
    wait.wait.seconds = Some(runinator_models::workflows::WorkflowWaitSeconds::Integer(1));
    let module = compile_with_metadata(
        "interrupt-and-resume-nodes",
        vec![
            node("start", WorkflowNodeKind::Start, Some("wait")),
            wait,
            node("end", WorkflowNodeKind::End, None),
            handler,
            handler_note,
            resume,
        ],
        runinator_models::json!({
            "interrupts": [{ "on": "external", "handler": "handler" }]
        }),
    );
    let mut main = continuation(&module);
    main.pending_interrupt = Some(WorkflowPendingInterrupt {
        id: Uuid::now_v7(),
        source: InterruptSource::External,
        payload: runinator_models::json!({ "reason": "operator request" }),
    });
    let WorkflowVmStep::Interrupted {
        mut suspended,
        handler,
        source,
    } = step_workflow_vm(&module, main)
    else {
        panic!("the wait node safe point must raise the declared interrupt");
    };
    assert_eq!(source, InterruptSource::External);
    assert_eq!(suspended.status, WorkflowContinuationStatus::Suspended);

    let WorkflowVmStep::InterruptResolved {
        handler,
        interrupted_continuation_id,
        outcome,
    } = step_workflow_vm(&module, *handler)
    else {
        panic!("the interrupt handler must finish at its resume node");
    };
    assert_eq!(interrupted_continuation_id, suspended.id);
    assert_eq!(handler.status, WorkflowContinuationStatus::Succeeded);
    let WorkflowInterruptOutcome::Resume {
        instruction_pointer,
    } = outcome
    else {
        panic!("resume mode must make the interrupted continuation runnable");
    };

    // This is the state transition the durable host performs after persisting the handler result.
    // Driving it here proves `resume` returned to the wait effect rather than skipping or
    // replaying the interrupt safe point.
    suspended.status = WorkflowContinuationStatus::Runnable;
    suspended.instruction_pointer = instruction_pointer;
    let WorkflowVmStep::Yield {
        continuation,
        request,
        ..
    } = step_workflow_vm(&module, suspended)
    else {
        panic!("the resumed main flow must issue its original wait");
    };
    assert_eq!(*request, WorkflowEffectRequest::TimerDelay { seconds: 1 });
    let WorkflowVmStep::Complete { value, .. } = resume_workflow_vm(
        &module,
        continuation,
        Some(&request),
        Ok(Value::String("woke".into())),
    ) else {
        panic!("the settled wait must still reach end");
    };
    assert_eq!(value, Value::String("woke".into()));
}

#[test]
fn node_suite_tracks_every_author_facing_vm_node_kind() {
    // Keep this ledger beside the executable scenarios above.  Adding a node kind now fails this
    // test until its data/lifecycle case is added to the suite, rather than silently expanding the
    // catalog without VM coverage.
    let covered = [
        WorkflowNodeKind::Start,
        WorkflowNodeKind::Action,
        WorkflowNodeKind::Wait,
        WorkflowNodeKind::Condition,
        WorkflowNodeKind::Switch,
        WorkflowNodeKind::Toggle,
        WorkflowNodeKind::Percentage,
        WorkflowNodeKind::Approval,
        WorkflowNodeKind::Gate,
        WorkflowNodeKind::Signal,
        WorkflowNodeKind::Loop,
        WorkflowNodeKind::Parallel,
        WorkflowNodeKind::Join,
        WorkflowNodeKind::Try,
        WorkflowNodeKind::Map,
        WorkflowNodeKind::Race,
        WorkflowNodeKind::Output,
        WorkflowNodeKind::Input,
        WorkflowNodeKind::Subflow,
        WorkflowNodeKind::Config,
        WorkflowNodeKind::Assert,
        WorkflowNodeKind::Transform,
        WorkflowNodeKind::Audit,
        WorkflowNodeKind::Checkpoint,
        WorkflowNodeKind::Mutex,
        WorkflowNodeKind::Throttle,
        WorkflowNodeKind::Cooldown,
        WorkflowNodeKind::AwaitRun,
        WorkflowNodeKind::Debounce,
        WorkflowNodeKind::Collect,
        WorkflowNodeKind::Barrier,
        WorkflowNodeKind::CircuitBreaker,
        WorkflowNodeKind::EventSource,
        WorkflowNodeKind::Invocation,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
        WorkflowNodeKind::Interrupt,
        WorkflowNodeKind::Resume,
    ];
    assert_eq!(covered, WorkflowNodeKind::ALL);
}

#[path = "workflow_vm_action_output_tests.rs"]
mod action_output_tests;
