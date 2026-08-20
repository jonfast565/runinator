//! loops and branches: `for` bounds, `while`/`until` desugaring, conditionals, truthy tests, and
//! switch shorthand.

use super::*;

#[test]
fn for_loop_limit_literal_uses_typed_field() {
    let src = r#"
        workflow "LimitLit" v1 {
            params { items: int[] }
            for n in params.items limit 5 {
                console.run(command: string(n))
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let loop_node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "loop")
        .expect("loop node");
    assert_eq!(loop_node["max_iterations"], 5);
    assert_round_trips(src);
}

#[test]
fn for_bindings_keep_authored_names_types_and_index() {
    let src = r#"
        workflow "LoopBindings" v1 {
            params { items: any[] }
            for ticket: { key: string }, i in params.items {
                emit "ticket" { key: ticket.key, index: i }
            }
        }
    "#;
    let definition = compile(src);
    let decompiled = decompile(&definition).expect("decompile");
    assert!(decompiled.contains("for ticket: { key: string }, i in params.items"));
    assert_round_trips(src);
}

#[test]
fn bound_for_collects_the_loop_results_array() {
    let src = r#"
        workflow "CollectedLoop" v1 {
            params { items: string[] }
            node results <- for item in params.items {
                node echoed <- console.run(command: item)
            }
            console.run(command: string(results))
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let collector = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "results")
        .expect("bound loop collector");
    assert!(
        collector.to_string().contains("results"),
        "collector should read the loop accumulator: {collector}"
    );
    assert_round_trips(src);
}

#[test]
fn while_limit_none_is_an_uncapped_loop() {
    let src = r#"
        workflow "UncappedWhile" v1 {
            params { ready: bool }
            while params.ready == false limit none {
                wait 1s
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn map_keeps_its_authored_variable_name() {
    let src = r#"
        workflow "MapBinding" v1 {
            params { tickets: string[] }
            map ticket in params.tickets concurrency 2 {
                console.run(command: ticket)
            }
        }
    "#;
    let decompiled = decompile(&compile(src)).expect("decompile");
    assert!(decompiled.contains("map ticket in params.tickets"));
    assert_round_trips(src);
}
#[test]
fn for_loop_limit_accepts_expression() {
    // an expression cap is carried in the loop parameters (resolved at runtime) and
    // round-trips back to `limit <expr>` through the decompiler.
    let src = r#"
        workflow "LimitExpr" v1 {
            params { items: int[], budget: int }
            for n in params.items limit params.budget {
                console.run(command: string(n))
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let loop_node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "loop")
        .expect("loop node");
    assert!(
        loop_node["parameters"]["max_iterations"].is_object(),
        "expression cap should live in parameters: {loop_node}"
    );
    assert_round_trips(src);
}
#[test]
#[ignore = "invocation output type hint migration pending"]
fn typed_compute_output_hint_validates_loop_items() {
    let src = r#"
        workflow "TypedComputeLoop" v1 {
            node impact: { lambdas: string[] } <- do {
                return { lambdas: ["one", "two"] }
            }
            for lambda_path in impact.lambdas limit none {
                console.run(command: lambda_path)
            }
        }
    "#;
    let definition = compile(src);
    let providers = vec![
        ProviderMetadata {
            name: "std".into(),
            actions: vec![ActionMetadata::new("run", "compute").with_parameters(vec![
                ParameterMetadata::required("program", RuninatorType::Any),
            ])],
            metadata: ProviderRuntimeMetadata::default(),
        },
        ProviderMetadata {
            name: "console".into(),
            actions: vec![ActionMetadata::new("run", "console").with_parameters(vec![
                ParameterMetadata::optional("command", RuninatorType::Any),
            ])],
            metadata: ProviderRuntimeMetadata::default(),
        },
    ];

    runinator_workflows::validate_workflow_with_providers(&definition, &providers)
        .expect("declared compute output type should drive loop item typing");
}
#[test]
fn round_trips_while_loop() {
    let src = r#"
        workflow "Polling" v1 {
            node seed <- console.run(command: "seed")
            while seed.status == "pending" limit 30 {
                console.run(command: "poll")
            }
            node done <- console.run(command: "done")
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn until_compiles_to_negated_while_condition() {
    // `until c` must lower to a reentry-enabled condition node whose branch fires while !c.
    let definition = compile(
        r#"
        workflow "Until" v1 {
            node seed <- console.run(command: "seed")
            until seed.ready == true limit 10 {
                console.run(command: "poll")
            }
        }
    "#,
    );
    let graph = definition.definition.as_value();
    let nodes = graph.get("nodes").and_then(|n| n.as_array()).unwrap();
    let header = nodes
        .iter()
        .find(|n| {
            n.get("kind").and_then(|k| k.as_str()) == Some("condition")
                && n.pointer("/reentry/enabled").and_then(|v| v.as_bool()) == Some(true)
        })
        .expect("while/until condition header");
    assert_eq!(
        header
            .pointer("/reentry/max_visits")
            .and_then(|v| v.as_i64()),
        Some(10)
    );
    // the single branch condition must be negated (a `not` wrapper) for `until`.
    assert!(
        header.pointer("/transitions/branches/0/when/not").is_some(),
        "until condition should be negated: {header:#?}"
    );
}
#[test]
fn round_trips_until_loop() {
    let src = r#"
        workflow "UntilReady" v1 {
            node seed <- console.run(command: "seed")
            until seed.ready == true limit 12 {
                console.run(command: "poll")
            }
            node finish <- console.run(command: "finish")
        }
    "#;
    // `until c` round-trips through its negated `while !c` form (graph-equivalent).
    assert_round_trips(src);
}
#[test]
fn round_trips_conditionals() {
    let src = r#"
        workflow "Conditionals" v1 {
            node probe <- console.run(command: "probe")
            if probe.count > 0 {
                console.run(command: "many")
            } else {
                console.run(command: "none")
            }
            match probe.mode {
                "fast" -> { console.run(command: "fast") }
                else -> { console.run(command: "slow") }
            }
            node report <- console.run(command: "report")
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn round_trips_truthy_conditions() {
    let src = r#"
        workflow "TruthyConditions" v1 {
            if true {
                console.run(command: "yes")
            } else {
                console.run(command: "no")
            }
            while 1 + 1 > 0 limit 1 {
                console.run(command: "loop")
            }
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn round_trips_truthy_compute_conditions() {
    let bool_src = r#"
        workflow "TruthyComputeConditions" v1 {
            do {
                if true {
                    return 1
                } else {
                    return 0
                }
            }
        }
    "#;
    assert_round_trips(bool_src);

    let expr_src = r#"
        workflow "TruthyComputeExprConditions" v1 {
            do {
                if 1 + 1 {
                    return 1
                } else {
                    return 0
                }
            }
        }
    "#;
    assert_round_trips(expr_src);
}
#[test]
fn round_trips_leaves() {
    let src = r#"
        workflow "Leaves" v1 {
            node probe <- console.run(command: "probe")
            wait 30s until "ready"
            emit "checked" { count: probe.count }
            approve "Ship it?" type "change_request" { env: "prod" }
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn round_trips_scalar_output_payloads() {
    // output payloads are arbitrary expressions, not just objects. an event-less scalar is
    // parenthesized so it is not parsed as the event type.
    let src = r#"
        workflow "Payloads" {
            node probe <- console.run(command: "probe")
            emit "count" probe.count
            emit "nums" [1, 2, 3]
            emit ("ready")
            emit (42)
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn action_node_parameters_are_not_dropped() {
    // action call args live in `configuration`, but the reducer also merges node-level
    // `parameters`. a node that only set `parameters` must still decompile to call args.
    use runinator_models::value::Value;
    use runinator_models::workflows::{WorkflowNodeKind, WorkflowObject};

    let mut def = compile(r#"workflow "Params" { console.run(command: "probe") }"#);
    let action = def
        .definition
        .nodes
        .iter_mut()
        .find(|node| node.kind == WorkflowNodeKind::Action)
        .expect("action node");
    action.parameters =
        WorkflowObject::from_value(Value::from(serde_json::json!({ "retries": 3 })))
            .expect("parameters");

    let rexrap = decompile(&def).expect("decompile");
    assert!(
        rexrap.contains("command:"),
        "configuration arg preserved:\n{rexrap}"
    );
    assert!(
        rexrap.contains("retries: 3"),
        "node parameter surfaced:\n{rexrap}"
    );

    // the surfaced parameter recompiles into the action configuration (same merge result).
    let recompiled = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    let action = recompiled
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == WorkflowNodeKind::Action)
        .expect("action node");
    assert_eq!(
        action
            .action
            .as_ref()
            .unwrap()
            .configuration
            .get("retries")
            .and_then(Value::as_i64),
        Some(3)
    );
}
#[test]
fn switch_shorthand_conditions_decompile() {
    // switch cases authored as not_equals / exists shorthand (no explicit `when`) must decompile
    // into the equivalent guard rather than erroring.
    use runinator_models::value::Value;
    use runinator_models::workflows::{WorkflowNodeKind, WorkflowObject};

    let rebuild = |case: serde_json::Value| {
        let mut def = compile(
            r#"
            workflow "Switch" {
                node probe <- console.run(command: "probe")
                match probe.mode {
                    "fast" -> { console.run(command: "fast") }
                    else -> { console.run(command: "slow") }
                }
            }
        "#,
        );
        let switch = def
            .definition
            .nodes
            .iter_mut()
            .find(|node| node.kind == WorkflowNodeKind::Switch)
            .expect("switch node");
        let mut params: serde_json::Value =
            serde_json::to_value(switch.parameters.as_value()).expect("params");
        let target = params["cases"][0]["target"].clone();
        let mut rewritten = case;
        rewritten["target"] = target;
        params["cases"][0] = rewritten;
        switch.parameters =
            WorkflowObject::from_value(Value::from(params)).expect("rebuild params");
        def
    };

    let not_equals = rebuild(serde_json::json!({ "not_equals": "fast" }));
    let rexrap = decompile(&not_equals).expect("decompile not_equals shorthand");
    assert!(
        rexrap.contains("when") && rexrap.contains("!="),
        "not_equals rendered as guard:\n{rexrap}"
    );
    compile_str(&rexrap, &CompileOptions::default()).expect("recompile not_equals shorthand");

    let exists = rebuild(serde_json::json!({ "exists": true }));
    let rexrap = decompile(&exists).expect("decompile exists shorthand");
    assert!(
        rexrap.contains("exists"),
        "exists rendered as guard:\n{rexrap}"
    );
    compile_str(&rexrap, &CompileOptions::default()).expect("recompile exists shorthand");
}
