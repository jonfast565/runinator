//! the node kinds themselves: every kind compiles and round-trips, optional clauses included,
//! plus mutex sections and disconnected nodes.

use super::*;

/// the editor regenerates the rexrap pane via `decompile` on every refresh/save, so `decompile`
/// output must already be in the formatter's canonical shape or a user's `Format` silently
/// reverts. this guards the struct-type-in-params case that originally diverged.
#[test]
fn decompile_output_is_format_idempotent() {
    let samples: &[&str] = &[
        r#"workflow "Core Team SDLC Pipeline" v1 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
            }
            node tickets <- jira.search(jql: params.jira.jql).timeout(120s).retry(3)
            for ticket in tickets.issues limit 50 {
                subflow("Ticket Work", params: { ticket, parent_workflow_run_id: run.run_id }, detached: true, reuse: true, name: "Ticket Work: ${ticket.key}")
            }
        }"#,
        r#"workflow "Concurrency" v1 {
            node probe <- console.run(command: "probe")
            parallel {
                branch { console.run(command: "lint") }
                branch { console.run(command: "test") }
            } join all
            node report <- console.run(command: "report")
        }"#,
    ];
    for src in samples {
        let decompiled = decompile(&compile(src)).expect("decompile");
        let formatted = format_str(&decompiled).expect("format");
        assert_eq!(
            decompiled, formatted,
            "decompile output is not format-stable:\n--- decompiled ---\n{decompiled}\n--- formatted ---\n{formatted}"
        );
    }
}

#[test]
fn selected_parallel_join_leaves_unselected_branches_on_private_terminals() {
    use runinator_models::workflows::WorkflowNodeKind;

    let src = r#"
        workflow "Selected Parallel" v1 {
            parallel {
                branch "lint" { console.run(command: "lint") }
                branch "tests" { console.run(command: "test") }
                branch "security" { console.run(command: "security") }
            } join ["lint", "tests"] all
            console.run(command: "publish")
        }
    "#;
    let definition = compile(src);
    let graph = &definition.definition;
    let parallel = graph
        .nodes
        .iter()
        .find(|node| node.kind == WorkflowNodeKind::Parallel)
        .expect("parallel node");
    let join = graph
        .nodes
        .iter()
        .find(|node| node.kind == WorkflowNodeKind::Join)
        .expect("join node");
    let branch_ids = parallel
        .parameters
        .get("branches")
        .and_then(Value::as_array)
        .expect("parallel branches");
    let wait_for = join
        .parameters
        .get("wait_for")
        .and_then(Value::as_array)
        .expect("join wait_for");
    assert_eq!(branch_ids.len(), 3);
    assert_eq!(wait_for.len(), 2);
    assert!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == WorkflowNodeKind::End)
            .count()
            >= 2
    );

    let decompiled = decompile(&definition).expect("decompile selected join");
    assert!(decompiled.contains("branch \"security\""));
    assert!(decompiled.contains("} join [\"lint\", \"tests\"] all"));
    assert_round_trips(src);
}
/// the twelve coordination/resilience/diagnostic node kinds each compile to the expected kind and
/// survive a compile -> decompile -> compile round trip.
#[test]
fn new_node_kinds_compile_and_round_trip() {
    use runinator_models::workflows::WorkflowNodeKind;

    let src = r#"
        workflow "New Nodes" v1 {
            params { run_ids: string[], amount: int, user: string }
            node seed <- console.run(command: "echo go")
            assert {
                "amount_positive": params.amount > 0
            }
            transform {
                doubled = params.amount * 2
            }
            audit action "reviewed" actor params.user
            checkpoint "after-audit"
            mutex "deploy-lock" every 5s timeout 300s
            throttle "github-api" rate 10 per 60s
            cooldown "scan-gate" every 300s
            await workflow "Prep" key params.user mode "all" timeout 1800s
            debounce "file-change" delay 30s
            collect "events" max 50 timeout 300s
            barrier "shard-sync" count 4 timeout 600s
            circuit_breaker "payment-api" threshold 5 window 60s cooldown 120s
            event_source type "file.uploaded" max 100 timeout 3600s
            node finish <- console.run(command: "echo done")
        }
    "#;

    let definition = compile(src);
    let kinds: Vec<_> = definition
        .definition
        .nodes
        .iter()
        .map(|n| n.kind.clone())
        .collect();
    for expected in [
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
    ] {
        assert!(kinds.contains(&expected), "missing node kind {expected:?}");
    }

    let cooldown = definition
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::Cooldown)
        .expect("cooldown node");
    assert_eq!(
        cooldown
            .parameters
            .get("window_seconds")
            .and_then(Value::as_i64),
        Some(300)
    );

    // spot-check a couple of lowered parameter shapes against what the reducer reads.
    let throttle = definition
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::Throttle)
        .expect("throttle node");
    assert_eq!(
        throttle
            .parameters
            .get("max_per_window")
            .and_then(Value::as_i64),
        Some(10)
    );
    assert_eq!(
        throttle
            .parameters
            .get("window_seconds")
            .and_then(Value::as_i64),
        Some(60)
    );
    let mutex = definition
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::Mutex)
        .expect("mutex node");
    assert_eq!(mutex.timeout_seconds, Some(300));

    let await_node = definition
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::AwaitRun)
        .expect("await node");
    assert_eq!(
        await_node
            .parameters
            .get("workflow")
            .and_then(Value::as_str),
        Some("Prep")
    );
    assert_eq!(
        await_node.parameters.get("mode").and_then(Value::as_str),
        Some("all")
    );
    assert!(await_node.parameters.get("key").is_some());
    assert_eq!(await_node.timeout_seconds, Some(1800));

    assert_round_trips_unordered(src);
}
/// every optional clause of the coordination/resilience node kinds survives a round trip. the
/// happy-path test above omits these (audit's actor/target/reason, an event_source filter, a
/// debounce key, the various `every` poll clauses), so they are exercised here one shape at a time.
#[test]
fn new_node_kinds_optional_clauses_round_trip() {
    let bodies = [
        (
            "audit-all-fields",
            "audit action \"reviewed\" actor params.user target \"acct\" reason \"policy\"",
        ),
        (
            "event_source-filter",
            "event_source type \"file.uploaded\" filter params.size > 1000 max 100 timeout 3600s",
        ),
        ("debounce-key", "debounce \"f\" delay 30s key params.user"),
        (
            "await-key-mode-timeout",
            "await workflow \"Prep\" key params.user mode \"any\" timeout 1800s",
        ),
        ("mutex-poll", "mutex \"deploy\" every 5s timeout 300s"),
        (
            "throttle-poll",
            "throttle \"gh\" rate 10 per 60s every 5s timeout 120s",
        ),
        ("cooldown", "cooldown \"scan-gate\" every 300s"),
        (
            "barrier-poll",
            "barrier \"sync\" count 4 every 5s timeout 600s",
        ),
        ("collect-timeout", "collect \"events\" max 50 timeout 300s"),
        (
            "circuit_breaker",
            "circuit_breaker \"api\" threshold 5 window 60s cooldown 120s",
        ),
        (
            "assert-multi",
            "assert {\n    \"positive\": params.amount > 0\n    \"bounded\": params.amount < 100\n  }",
        ),
        (
            "transform-multi",
            "transform {\n    doubled = params.amount * 2\n    who = params.user\n  }",
        ),
    ];
    for (label, body) in bodies {
        let src = format!(
            "workflow \"Opt\" v1 {{\n  params {{ run_ids: string[], amount: int, user: string, size: int }}\n  node seed <- console.run(command: \"echo go\")\n  {body}\n  node finish <- console.run(command: \"echo done\")\n}}\n"
        );
        let first = compile_str(&src, &default_test_options())
            .unwrap_or_else(|err| panic!("compile {label} failed: {err}"));
        let rexrap =
            decompile(&first).unwrap_or_else(|err| panic!("decompile {label} failed: {err}"));
        let second = compile_str(&rexrap, &default_test_options()).unwrap_or_else(|err| {
            panic!("recompile {label} failed: {err}\n--- rexrap ---\n{rexrap}")
        });
        assert_eq!(
            graph_value(&first),
            graph_value(&second),
            "{label} diverged on round trip\n--- rexrap ---\n{rexrap}"
        );
    }
}
/// a `mutex "name" { ... }` critical section lowers to an acquire node plus a paired release node,
/// and round-trips back to the block form (with `hold` preserved).
#[test]
fn mutex_block_lowers_and_round_trips() {
    let src = "workflow \"Crit\" v1 {\n  mutex \"deploy\" hold 300s {\n    node work <- console.run(command: \"echo go\")\n  }\n  node finish <- console.run(command: \"echo done\")\n}\n";
    let first = compile_str(src, &default_test_options()).expect("compile block");

    // the block produces a mutex acquire node and a paired mutex release node.
    let graph = graph_value(&first);
    let nodes = graph["nodes"].as_array().expect("nodes array");
    let mutexes: Vec<&serde_json::Value> = nodes
        .iter()
        .filter(|node| node["kind"] == "mutex")
        .collect();
    assert_eq!(mutexes.len(), 2, "expected acquire + release nodes");
    let acquire = mutexes
        .iter()
        .find(|node| node["parameters"]["release"] != serde_json::json!(true))
        .expect("acquire node");
    let release = mutexes
        .iter()
        .find(|node| node["parameters"]["release"] == serde_json::json!(true))
        .expect("release node");
    assert_eq!(acquire["parameters"]["name"], serde_json::json!("deploy"));
    assert_eq!(
        acquire["parameters"]["hold_timeout_seconds"],
        serde_json::json!(300)
    );
    assert_eq!(release["parameters"]["name"], serde_json::json!("deploy"));

    let rexrap = decompile(&first).expect("decompile block");
    assert!(
        rexrap.contains("mutex \"deploy\" hold 300s {"),
        "block form not reconstructed:\n{rexrap}"
    );
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- rexrap ---\n{rexrap}"));
    assert_eq!(
        graph_value(&first),
        graph_value(&second),
        "mutex block diverged on round trip\n--- rexrap ---\n{rexrap}"
    );
}
/// a bare `mutex release "name"` leaf lowers to a release node and round-trips.
#[test]
fn mutex_release_leaf_round_trips() {
    let src = "workflow \"Rel\" v1 {\n  mutex \"deploy\"\n  node work <- console.run(command: \"echo go\")\n  mutex release \"deploy\"\n  node finish <- console.run(command: \"echo done\")\n}\n";
    let first = compile_str(src, &default_test_options()).expect("compile release leaf");
    let rexrap = decompile(&first).expect("decompile release leaf");
    assert!(
        rexrap.contains("mutex release \"deploy\""),
        "release leaf not rendered:\n{rexrap}"
    );
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- rexrap ---\n{rexrap}"));
    assert_eq!(
        graph_value(&first),
        graph_value(&second),
        "mutex release leaf diverged on round trip\n--- rexrap ---\n{rexrap}"
    );
}
/// a disconnected node (no incoming edge — e.g. one just added in the editor before the author
/// wires it) must still appear in the decompiled rexrap rather than silently vanishing.
#[test]
fn decompile_preserves_disconnected_node() {
    let definition: runinator_models::workflows::WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "name": "Orphan",
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "lonely", "kind": "mutex", "parameters": { "name": "deploy-lock" }, "transitions": {} },
                { "id": "end", "kind": "end" }
            ]
        }
    }))
    .expect("deserialize definition");

    let rexrap = decompile(&definition).expect("decompile");
    assert!(
        rexrap.contains("mutex \"deploy-lock\""),
        "disconnected node dropped from decompiled rexrap:\n{rexrap}"
    );
    // and the node id is preserved via the `@id(...)` annotation so re-import is stable.
    assert!(
        rexrap.contains("@id(\"lonely\")"),
        "missing id annotation:\n{rexrap}"
    );

    let recompiled = compile_str(&rexrap, &default_test_options()).expect("recompile");
    assert!(
        recompiled
            .definition
            .nodes
            .iter()
            .any(|node| node.id == "lonely"),
        "disconnected node lost after round trip"
    );
}

// comment preservation (lossless formatting) --------------------------------

// assert that `needle` appears in `haystack` and returns its byte offset, for ordering checks.
