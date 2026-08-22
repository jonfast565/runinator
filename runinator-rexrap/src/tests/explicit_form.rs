//! decompiling back to source: what the explicit form surfaces that the terse form leaves
//! implicit, and the ids and edges a round trip must not lose.

use super::*;

#[test]
fn explicit_decompile_surfaces_loop_edges_and_none_caps() {
    // a for-loop with no limit: the back-edge, the continuation, the block id, and `limit none`.
    let rexrap = assert_round_trips_explicit(
        r#"
        workflow "Loop" v1 {

            do {
                let seed = console.run(command: "seed")
                for item in seed.items {
                    console.run(command: "work ${item}")
                }
                map shard in seed.shards {
                    console.run(command: "reindex ${shard}")
                }
            }
        }
    "#,
    );
    assert!(
        rexrap.contains("limit none"),
        "missing explicit for cap:\n{rexrap}"
    );
    assert!(
        rexrap.contains("concurrency none"),
        "missing explicit map cap:\n{rexrap}"
    );
    assert!(
        rexrap.contains("@id("),
        "missing control-block id:\n{rexrap}"
    );
    assert!(
        rexrap.contains("on next {"),
        "missing block continuation route:\n{rexrap}"
    );
}
#[test]
fn terse_decompile_preserves_authored_control_block_id() {
    let first = compile(
        r#"
        workflow "LoopId" v1 {

            do {
                let seed = console.run(command: "seed")
                @id("for_each_ticket")
                for item in seed.items {
                    console.run(command: "work")
                }
            }
        }
    "#,
    );
    assert_eq!(
        first.definition.metadata.pointer("/rexrap/control_ids/0"),
        Some(&Value::from("for_each_ticket"))
    );

    let rexrap = decompile(&first).expect("decompile");
    assert!(
        rexrap.contains(r#"@id("for_each_ticket")"#),
        "missing authored control id:\n{rexrap}"
    );
    assert!(
        !rexrap.contains("limit none"),
        "terse decompile should not become fully explicit:\n{rexrap}"
    );
    let second = compile_str(&rexrap, &default_test_options()).expect("recompile");
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition),
        "control id round trip diverged:\n{rexrap}"
    );
}
#[test]
fn terse_decompile_preserves_imported_control_block_id() {
    use runinator_models::workflows::WorkflowDefinition;

    let definition = compile(
        r#"
        workflow "ImportedLoopId" v1 {

            do {
                let seed = console.run(command: "seed")
                for item in seed.items {
                    console.run(command: "work")
                }
            }
        }
    "#,
    );
    let old_id = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Loop)
        .map(|node| node.id.clone())
        .expect("loop node");
    let mut value = serde_json::to_value(&definition).expect("serialize definition");
    replace_node_id(&mut value, &old_id, "for_each_ticket");
    value["definition"]["metadata"] = serde_json::json!({});
    let imported: WorkflowDefinition = serde_json::from_value(value).expect("rebuild definition");

    let rexrap = decompile(&imported).expect("decompile");
    assert!(
        rexrap.contains(r#"@id("for_each_ticket")"#),
        "missing imported control id:\n{rexrap}"
    );
    let recompiled = compile_str(&rexrap, &default_test_options()).expect("recompile");
    assert!(
        recompiled
            .definition
            .nodes
            .iter()
            .any(|node| node.id == "for_each_ticket"),
        "recompiled graph did not preserve loop id:\n{rexrap}"
    );
}
#[test]
fn explicit_and_implicit_caps_are_equivalent() {
    // `limit none` / `concurrency none` must compile to the same graph as omitting them.
    let explicit = compile(
        r#"
        workflow "Caps" v1 {

            do {
                let seed = console.run(command: "seed")
                for x in seed.items limit none { console.run(command: "a ${x}") }
                map y in seed.items concurrency none { console.run(command: "b ${y}") }
            }
        }
    "#,
    );
    let implicit = compile(
        r#"
        workflow "Caps" v1 {

            do {
                let seed = console.run(command: "seed")
                for x in seed.items { console.run(command: "a ${x}") }
                map y in seed.items { console.run(command: "b ${y}") }
            }
        }
    "#,
    );
    assert_eq!(
        runinator_workflows::normalize_definition(explicit.definition),
        runinator_workflows::normalize_definition(implicit.definition),
    );
}
#[test]
fn explicit_start_and_next_arrows_parse_and_match_implicit() {
    // an explicit `start ->` plus `on next`/`on success` routes must produce the same graph as the
    // implicit sequence they spell out.
    let explicit = compile(
        r#"
        workflow "Explicit" v1 {
            start -> first

            do {
                @id("first") wait 5s
                    routes {
                        on next {
                            continue second
                        }
                    }
                @id("second") console.run(command: "go")
                    routes {
                        on success {
                            continue end
                        }
                    }
            }
        }
    "#,
    );
    let implicit = compile(
        r#"
        workflow "Explicit" v1 {

            do {
                @id("first") wait 5s
                @id("second") console.run(command: "go")
            }
        }
    "#,
    );
    assert_eq!(
        runinator_workflows::normalize_definition(explicit.definition),
        runinator_workflows::normalize_definition(implicit.definition),
    );
}
#[test]
fn explicit_start_target_must_resolve() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            start -> ghost

            do {
                console.run(command: "x")
            }
        }
    "#,
    );
    assert!(message.contains("unknown step 'ghost'"), "{message}");
}
#[test]
fn explicit_round_trips_control_flow() {
    // every control construct survives the explicit form's always-on ids, arrows, and defaults.
    assert_round_trips_explicit(
        r#"
        workflow "Control" v1 {

            do {
                let probe = console.run(command: "probe")
                if probe.count > 0 {
                    console.run(command: "many")
                } else {
                    console.run(command: "none")
                }
                while probe.status == "pending" limit 30 {
                    console.run(command: "poll")
                }
                match probe.mode {
                    "fast" -> { console.run(command: "fast") }
                    else -> { console.run(command: "slow") }
                }
                parallel {
                    branch { console.run(command: "a") }
                    branch { console.run(command: "b") }
                } join all
                approve "ship?" { env: "prod" }
                let report = console.run(command: "report")
            }
        }
    "#,
    );
}
#[test]
fn gate_node_round_trips_each_kind() {
    let src = r#"
        workflow "Gated" v1 {

            do {
                let build = console.run(command: "build")
                gate condition when build.status == "ready" every 15s timeout 300s on_timeout continue
                gate manual { label: "release" }
                gate external every 60s
                let report = console.run(command: "report")
            }
        }
    "#;
    let definition = compile(src);
    let gates: Vec<_> = definition
        .definition
        .nodes
        .iter()
        .filter(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Gate)
        .collect();
    assert_eq!(gates.len(), 3, "expected three gate nodes");
    let condition_gate = gates
        .iter()
        .find(|node| node.parameters.get("kind").and_then(Value::as_str) == Some("condition"))
        .expect("condition gate");
    assert!(
        condition_gate.parameters.get("when").is_some(),
        "condition gate keeps its when"
    );
    assert_eq!(
        condition_gate
            .parameters
            .get("poll_interval")
            .and_then(Value::as_i64),
        Some(15)
    );
    assert_eq!(
        condition_gate
            .parameters
            .get("timeout")
            .and_then(Value::as_i64),
        Some(300)
    );
    assert_eq!(
        condition_gate
            .parameters
            .get("timeout_policy")
            .and_then(Value::as_str),
        Some("continue")
    );
    assert_round_trips(src);
}
#[test]
fn signal_node_round_trips() {
    let src = r#"
        workflow "Signalled" v1 {

            do {
                let build = console.run(command: "build")
                signal "deploy-approved" { source: "ops" }
                let ship = console.run(command: "ship")
            }
        }
    "#;
    let definition = compile(src);
    let signal = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Signal)
        .expect("signal node");
    assert_eq!(
        signal.parameters.get("name").and_then(Value::as_str),
        Some("deploy-approved")
    );
    assert_round_trips(src);
}
#[test]
fn output_node_artifact_round_trips() {
    let src = r#"
        workflow "Reports" v1 {

            do {
                let dump = console.run(command: "dump")
                output {
                    report = dump.artifacts
                    first = dump.artifacts[0]
                }
            }
        }
    "#;
    let definition = compile(src);
    let output_node = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Output)
        .expect("output node");
    let items = output_node
        .parameters
        .get("items")
        .and_then(Value::as_array)
        .expect("items array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].get("name").and_then(Value::as_str), Some("report"));
    assert_round_trips(src);
}
#[test]
fn predicate_edges_round_trip_with_priority() {
    let src = r#"
        workflow "Edges" v1 {
            params { status: string }

            do {
                let check = console.run(command: "check")
                routes {
                    on success {
                        continue end
                    }
                    when params.status == "approved" priority 1 {
                        continue review
                    }
                    when params.status == "denied" priority 2 {
                        continue reject
                    }
                }
                let review = console.run(command: "review")
                let reject = console.run(command: "reject")
            }
        }
    "#;
    let definition = compile(src);
    let check = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "check")
        .expect("check node");
    let branches = &check.transitions.branches;
    assert_eq!(branches.len(), 2, "predicate edges lower to branches");
    assert_eq!(branches[0].priority, Some(1));
    assert_eq!(branches[0].target.as_str(), "review");
    assert_eq!(branches[1].priority, Some(2));
    assert_eq!(branches[1].target.as_str(), "reject");
    assert_round_trips_unordered(src);
}
#[test]
fn predicate_edge_without_priority_round_trips() {
    let src = r#"
        workflow "Edges" v1 {
            params { status: string }

            do {
                let check = console.run(command: "check")
                routes {
                    when params.status == "skip" {
                        continue end
                    }
                }
                let after = console.run(command: "after")
            }
        }
    "#;
    let definition = compile(src);
    let check = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "check")
        .expect("check node");
    let branches = &check.transitions.branches;
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].priority, None);
    assert_round_trips_unordered(src);
}
#[test]
fn decompile_renders_back_edge_as_arrow_without_panicking() {
    use runinator_models::workflows::WorkflowDefinition;
    // a linear workflow whose graph we mutate to add a back-edge from `b` to `a`.
    let definition = compile(
        r#"
        workflow "Poller" v1 {

            do {
                let a = console.run(command: "a")
                let b = console.run(command: "b")
            }
        }
    "#,
    );
    let mut value = serde_json::to_value(&definition).expect("serialize definition");
    let nodes = value["definition"]["nodes"]
        .as_array_mut()
        .expect("nodes array");
    for node in nodes.iter_mut() {
        if node["id"] == serde_json::json!("b") {
            node["transitions"]["next"] = serde_json::json!({ "$node": "a" });
            node["transitions"]["on_success"] = serde_json::json!({ "$node": "a" });
        }
    }
    let looped: WorkflowDefinition = serde_json::from_value(value).expect("rebuild definition");
    // the back-edge must decompile to an explicit `-> a` arrow, never a crash or error.
    let rexrap = decompile(&looped).expect("decompile renders the back-edge");
    assert!(
        rexrap.contains("continue a"),
        "expected a back-edge route, got:\n{rexrap}"
    );
}
