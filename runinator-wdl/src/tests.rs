use crate::{
    CompileOptions, DecompileOptions, WdlCompletionRequest, WdlCompletionResponse, WdlError,
    WdlFragmentKind, analyze_source, compile_str, compile_str_with_diagnostics, complete_source,
    decompile, decompile_with, evaluate_fragment, format_str, parse_document, validate_fragment,
};
use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, ResultMetadata,
    RuninatorType,
};
use runinator_models::value::Value;
use std::{fs, time::SystemTime};

/// compile and return the `Semantic` error's span and message, failing otherwise.
fn expect_semantic(src: &str) -> (crate::Span, String) {
    match compile_str(src, &CompileOptions::default()) {
        Err(WdlError::Semantic { span, message }) => (span, message),
        other => panic!("expected semantic error, got {other:?}"),
    }
}

fn compile(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    compile_str(src, &CompileOptions::default()).expect("compile")
}

fn action_config_value<'a>(
    definition: &'a runinator_models::workflows::WorkflowDefinition,
    key: &str,
) -> &'a Value {
    definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .and_then(|node| node.action.as_ref())
        .and_then(|action| action.configuration.get(key))
        .unwrap_or_else(|| panic!("missing action configuration key '{key}'"))
}

fn graph_value(definition: &runinator_models::workflows::WorkflowDefinition) -> serde_json::Value {
    serde_json::to_value(&definition.definition).expect("serialize graph")
}

#[test]
fn lists_included_file_paths() {
    let src = r#"
        workflow "Includes" v1 {
            alias shared = { script: file("scripts/shared.py") }
            node go = console.run(command: file("scripts/job.py"), ...shared)
        }
    "#;
    let mut paths =
        crate::included_file_paths(src, std::path::Path::new("/pack")).expect("include paths");
    paths.sort();
    assert_eq!(
        paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["/pack/scripts/job.py", "/pack/scripts/shared.py"]
    );
}

fn completion_labels(src: &str, marker: &str) -> Vec<String> {
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    complete_source(WdlCompletionRequest {
        source,
        cursor_byte: cursor,
        providers: completion_providers(),
        settings: Vec::new(),
    })
    .items
    .into_iter()
    .map(|item| item.label)
    .collect()
}

fn completion_providers() -> Vec<ProviderMetadata> {
    let issue_type = RuninatorType::open_structure(
        [
            ("key", RuninatorType::String),
            (
                "fields",
                RuninatorType::open_structure(
                    [("summary", RuninatorType::String)],
                    RuninatorType::Any,
                ),
            ),
        ],
        RuninatorType::Any,
    );
    vec![
        ProviderMetadata {
            name: "jira".into(),
            actions: vec![
                ActionMetadata::new("search", "Search Jira issues")
                    .with_parameters(vec![
                        ParameterMetadata::required("base_url", RuninatorType::String),
                        ParameterMetadata::required("token", RuninatorType::String).secret(),
                        ParameterMetadata::optional("email", RuninatorType::String),
                        ParameterMetadata::required("jql", RuninatorType::String),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("issues", RuninatorType::array(issue_type)),
                        ResultMetadata::new("total", RuninatorType::Integer),
                    ]),
                ActionMetadata::new("transition", "Transition a Jira issue").with_parameters(vec![
                    ParameterMetadata::required("key", RuninatorType::String),
                ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        },
        ProviderMetadata {
            name: "slack".into(),
            actions: vec![ActionMetadata::new("send_message", "Send a Slack message")],
            metadata: ProviderRuntimeMetadata::default(),
        },
    ]
}

/// compile and require a semantic error, returning its message.
fn expect_semantic_error(src: &str) -> String {
    match compile_str(src, &CompileOptions::default()) {
        Err(WdlError::Semantic { message, .. }) => message,
        other => panic!("expected semantic error, got {other:?}"),
    }
}

/// whether `first` and `second` both appear in `text` with `first` preceding `second`. used by
/// layout-tolerant assertions now that arguments lay out one per line.
fn ordered(text: &str, first: &str, second: &str) -> bool {
    match (text.find(first), text.find(second)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

/// compile -> decompile -> compile and assert the normalized graphs match.
fn assert_round_trips(src: &str) {
    let first = compile(src);
    let wdl = decompile(&first).expect("decompile");
    let second = compile_str(&wdl, &CompileOptions::default())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{wdl}"));
    let normalized_first = runinator_workflows::normalize_definition(first.definition.clone());
    let normalized_second = runinator_workflows::normalize_definition(second.definition.clone());
    assert_eq!(
        normalized_first, normalized_second,
        "round trip diverged\n--- decompiled ---\n{wdl}"
    );
}

/// like `assert_round_trips`, but compares the node *set* rather than array order. node order
/// carries no execution meaning (the graph is followed via `start` + transitions), and a
/// decompiler that re-nests branches legitimately renders nodes in a different order.
fn assert_round_trips_unordered(src: &str) {
    let first = compile(src);
    let wdl = decompile(&first).expect("decompile");
    let second = compile_str(&wdl, &CompileOptions::default())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{wdl}"));

    let sorted_nodes = |definition: &runinator_models::workflows::WorkflowGraph| {
        let normalized = runinator_workflows::normalize_definition(definition.clone());
        let value = serde_json::to_value(&normalized).expect("serialize graph");
        let mut nodes = value
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        nodes.sort_by(|a, b| {
            let id = |v: &serde_json::Value| {
                v.get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            id(a).cmp(&id(b))
        });
        (value.get("start").cloned(), nodes)
    };

    assert_eq!(
        sorted_nodes(&first.definition),
        sorted_nodes(&second.definition),
        "round trip diverged (order-insensitive)\n--- decompiled ---\n{wdl}"
    );
}

/// decompile in the explicit form, recompile, and assert the normalized graphs match. node
/// order carries no execution meaning, so this compares the node *set* like the unordered helper.
fn assert_round_trips_explicit(src: &str) -> String {
    let first = compile(src);
    let wdl = decompile_with(&first, &DecompileOptions { explicit: true }).expect("decompile");
    let second = compile_str(&wdl, &CompileOptions::default())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- explicit ---\n{wdl}"));

    let sorted_nodes = |definition: &runinator_models::workflows::WorkflowGraph| {
        let normalized = runinator_workflows::normalize_definition(definition.clone());
        let value = serde_json::to_value(&normalized).expect("serialize graph");
        let mut nodes = value
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        nodes.sort_by(|a, b| {
            let id = |v: &serde_json::Value| {
                v.get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            id(a).cmp(&id(b))
        });
        (value.get("start").cloned(), nodes)
    };

    assert_eq!(
        sorted_nodes(&first.definition),
        sorted_nodes(&second.definition),
        "explicit round trip diverged\n--- explicit ---\n{wdl}"
    );
    wdl
}

#[test]
fn explicit_decompile_surfaces_every_implicit_part() {
    // a single action whose terse form hides start, ids, the success edge, and the defaults.
    let wdl = assert_round_trips_explicit(
        r#"
        workflow "Hello" v1 {
            node greeting = console.run(command: "echo hi")
        }
    "#,
    );
    assert!(
        wdl.contains("start -> greeting"),
        "missing start edge:\n{wdl}"
    );
    assert!(
        wdl.contains(".timeout(60s)"),
        "missing default timeout:\n{wdl}"
    );
    assert!(wdl.contains(".retry(1)"), "missing default retry:\n{wdl}");
    assert!(wdl.contains("ok -> done"), "missing success arrow:\n{wdl}");
}

#[test]
fn retry_lowers_backoff_and_classification() {
    let definition = compile(
        r#"
        workflow "Retry" v1 {
            node go = console.run(command: "echo hi")
                .retry(4, backoff: 2s, max: 60s, jitter: true, on: failure)
        }
    "#,
    );
    let node = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .expect("action node");
    assert_eq!(node.retry.max_attempts, 4);
    assert_eq!(node.retry.backoff_base_seconds, 2);
    assert_eq!(node.retry.backoff_max_seconds, 60);
    assert!(node.retry.jitter);
    assert_eq!(
        node.retry.retry_on,
        runinator_models::workflows::WorkflowRetryClass::Failure
    );
}

#[test]
fn compensation_lowers_and_round_trips() {
    let definition = compile(
        r#"
        workflow "Saga" v1 {
            node deploy = console.run(command: "deploy")
                compensate console.run(command: "rollback")
            node verify = console.run(command: "verify")
        }
    "#,
    );
    let deploy = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "deploy")
        .expect("deploy node");
    let compensation = deploy.compensation.as_ref().expect("compensation present");
    assert_eq!(compensation.provider, "console");
    assert_eq!(compensation.function, "run");

    assert_round_trips_unordered(
        r#"
        workflow "Saga" v1 {
            node deploy = console.run(command: "deploy")
                compensate console.run(command: "rollback")
            node verify = console.run(command: "verify")
        }
    "#,
    );
}

#[test]
fn watch_guard_lowers_to_metadata_and_round_trips() {
    let definition = compile(
        r#"
        workflow "Watch" v1 {
            params { status: string }
            watch params.status != "In Review" -> handle_drift
            node work = console.run(command: "echo work")
            node handle_drift = console.run(command: "echo drift")
        }
    "#,
    );
    let watches = definition
        .definition
        .metadata
        .pointer("/watches")
        .and_then(|value| value.as_array())
        .expect("watches metadata");
    assert_eq!(watches.len(), 1);
    assert_eq!(
        watches[0].get("handler").and_then(|h| h.as_str()),
        Some("handle_drift")
    );
    assert!(watches[0].get("condition").is_some());

    assert_round_trips_unordered(
        r#"
        workflow "Watch" v1 {
            params { status: string }
            watch params.status != "In Review" -> handle_drift
            node work = console.run(command: "echo work")
            node handle_drift = console.run(command: "echo drift")
        }
    "#,
    );
}

#[test]
fn signal_correlation_key_lowers_and_round_trips() {
    let definition = compile(
        r#"
        workflow "Sig" v1 {
            params { ticket: { key: string } }
            node seed = console.run(command: "echo go")
            signal "github.review" key params.ticket.key
            node after = console.run(command: "echo done")
        }
    "#,
    );
    let signal = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Signal)
        .expect("signal node");
    assert!(
        signal.parameters.get("correlation_key").is_some(),
        "correlation_key not lowered into signal params"
    );

    assert_round_trips_unordered(
        r#"
        workflow "Sig" v1 {
            params { ticket: { key: string } }
            node seed = console.run(command: "echo go")
            signal "github.review" key params.ticket.key
            node after = console.run(command: "echo done")
        }
    "#,
    );
}

#[test]
fn wait_until_desugars_to_condition_poll_loop() {
    // the terse condition wait must compile to the same graph as the explicit poll loop.
    let sugar = compile(
        r#"
        workflow "WaitUntil" v1 {
            node seed = console.run(command: "echo go")
            wait until seed.status == "ready" every 15s
            node after = console.run(command: "echo done")
        }
    "#,
    );
    let explicit = compile(
        r#"
        workflow "WaitUntil" v1 {
            node seed = console.run(command: "echo go")
            until seed.status == "ready" {
                wait 15s
            }
            node after = console.run(command: "echo done")
        }
    "#,
    );
    assert_eq!(
        runinator_workflows::normalize_definition(sugar.definition),
        runinator_workflows::normalize_definition(explicit.definition),
        "wait-until sugar diverged from the explicit until-loop"
    );
}

#[test]
fn wait_until_defaults_interval() {
    // omitting `every` must still compile to a valid poll loop (default interval).
    let _ = compile(
        r#"
        workflow "WaitUntil" v1 {
            node seed = console.run(command: "echo go")
            wait until seed.status == "ready"
            node after = console.run(command: "echo done")
        }
    "#,
    );
}

#[test]
fn retry_config_round_trips() {
    assert_round_trips(
        r#"
        workflow "Retry" v1 {
            node go = console.run(command: "echo hi")
                .retry(4, backoff: 2s, max: 60s, jitter: true, on: failure)
        }
    "#,
    );
}

#[test]
fn explicit_decompile_surfaces_loop_edges_and_none_caps() {
    // a for-loop with no limit: the back-edge, the continuation, the block id, and `limit none`.
    let wdl = assert_round_trips_explicit(
        r#"
        workflow "Loop" v1 {
            node seed = console.run(command: "seed")
            for item in seed.items {
                node console.run(command: "work ${item}")
            }
            map shard in seed.shards {
                node console.run(command: "reindex ${shard}")
            }
        }
    "#,
    );
    assert!(
        wdl.contains("limit none"),
        "missing explicit for cap:\n{wdl}"
    );
    assert!(
        wdl.contains("concurrency none"),
        "missing explicit map cap:\n{wdl}"
    );
    assert!(wdl.contains("@id("), "missing control-block id:\n{wdl}");
    assert!(
        wdl.contains("} next -> "),
        "missing block continuation arrow:\n{wdl}"
    );
}

#[test]
fn explicit_and_implicit_caps_are_equivalent() {
    // `limit none` / `concurrency none` must compile to the same graph as omitting them.
    let explicit = compile(
        r#"
        workflow "Caps" v1 {
            node seed = console.run(command: "seed")
            for x in seed.items limit none { node console.run(command: "a ${x}") }
            map y in seed.items concurrency none { node console.run(command: "b ${y}") }
        }
    "#,
    );
    let implicit = compile(
        r#"
        workflow "Caps" v1 {
            node seed = console.run(command: "seed")
            for x in seed.items { node console.run(command: "a ${x}") }
            map y in seed.items { node console.run(command: "b ${y}") }
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
    // an explicit `start ->` plus `next ->`/`ok ->` arrows must produce the same graph as the
    // implicit sequence they spell out.
    let explicit = compile(
        r#"
        workflow "Explicit" v1 {
            start -> first
            @id("first") wait 5s
                next -> second
            @id("second") node console.run(command: "go")
                ok -> done
        }
    "#,
    );
    let implicit = compile(
        r#"
        workflow "Explicit" v1 {
            @id("first") wait 5s
            @id("second") node console.run(command: "go")
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
            node console.run(command: "x")
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
            node probe = console.run(command: "probe")
            if probe.count > 0 {
                node console.run(command: "many")
            } else {
                node console.run(command: "none")
            }
            while probe.status == "pending" limit 30 {
                node console.run(command: "poll")
            }
            match probe.mode {
                "fast" -> { node console.run(command: "fast") }
                else -> { node console.run(command: "slow") }
            }
            parallel {
                branch { node console.run(command: "a") }
                branch { node console.run(command: "b") }
            } join all
            approve "ship?" { env: "prod" }
            node report = console.run(command: "report")
        }
    "#,
    );
}

#[test]
fn gate_node_round_trips_each_kind() {
    let src = r#"
        workflow "Gated" v1 {
            node build = console.run(command: "build")
            gate condition when build.status == "ready" every 15s timeout 300s
            gate manual { label: "release" }
            gate external every 60s
            node report = console.run(command: "report")
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
    assert_round_trips(src);
}

#[test]
fn signal_node_round_trips() {
    let src = r#"
        workflow "Signalled" v1 {
            node build = console.run(command: "build")
            signal "deploy-approved" { source: "ops" }
            node ship = console.run(command: "ship")
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
fn deliverable_node_round_trips() {
    let src = r#"
        workflow "Reports" v1 {
            node dump = console.run(command: "dump")
            deliverable {
                report = dump.artifacts
                first = dump.artifacts[0]
            }
        }
    "#;
    let definition = compile(src);
    let deliverable = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Deliverable)
        .expect("deliverable node");
    let items = deliverable
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
            node check = console.run(command: "check")
            edges {
                ok -> done
                when params.status == "approved" priority 1 -> review
                when params.status == "denied" priority 2 -> reject
            }
            node review = console.run(command: "review")
            node reject = console.run(command: "reject")
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
            node check = console.run(command: "check")
            edges {
                when params.status == "skip" -> done
            }
            node after = console.run(command: "after")
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
            node a = console.run(command: "a")
            node b = console.run(command: "b")
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
    let wdl = decompile(&looped).expect("decompile renders the back-edge");
    assert!(
        wdl.contains("-> a"),
        "expected a back-edge arrow, got:\n{wdl}"
    );
}

#[test]
fn format_normalizes_wdl_source() {
    let src = r#"workflow "Fmt"   v1{params{jira:{base_url:string,email?:string}, "odd-key": map<string[]>, fallback?: string, enabled: boolean, retry: integer, transitions:{done:string,in_progress:string,in_review:string}}
@skip node first: { output: string, status: string, items: string[] } = console.run(command:"echo ${params.jira.base_url}"++(params.fallback??"none"), transitions:{done:"done",in_progress:"progress",in_review:"review"}).timeout(30s).retry(2).tags("ci","fmt").mcp()
fail -> cleanup
timeout -> fail
if params.enabled==true&&exists first.output{output "ready"{value:first.output}}else{wait 5s}
match first.status{"ok"->node console.run(command:"ok") when params.retry > 0 -> {node console.run(command:"retry")} else -> fail "bad"}
parallel{branch{node console.run(command:"a")}branch{node console.run(command:"b")}}join any
try{node console.run(command:"risky")}catch{node console.run(command:"recover")}finally{node console.run(command:"done")}
race winner first_success{branch{node console.run(command:"primary")}branch{node console.run(command:"backup")}}
map item in first.items concurrency 2{node console.run(command:string(item))}
node cleanup = console.run(command:"cleanup")
node jira.transition(base_url:params.jira.base_url,email:params.jira.email,key:first.output,token:"secret",transition_id:params.transitions.in_progress).timeout(30s)
}"#;

    let formatted = format_str(src).expect("format");
    let expected = r#"workflow "Fmt" v1.0.0 {
    params {
        jira: {
            base_url: string,
            email?: string
        }
        "odd-key": map<string[]>
        fallback?: string
        enabled: boolean
        retry: integer
        transitions: {
            done: string,
            in_progress: string,
            in_review: string
        }
    }

    @skip
    node first: { output: string, status: string, items: string[] } = console.run(
        command: "echo ${params.jira.base_url}" ++ (params.fallback ?? "none"),
        transitions: {
            done: "done",
            in_progress: "progress",
            in_review: "review"
        }
    ).timeout(30s)
     .retry(2)
     .tags("ci", "fmt")
     .mcp()
    edges {
        fail -> cleanup
        timeout -> fail
    }

    if params.enabled == true && exists first.output {
        output "ready" {
            value: first.output
        }
    } else {
        wait 5s
    }

    match first.status {
        "ok" -> {
            node console.run(
                command: "ok"
            )
        }
        when params.retry > 0 -> {
            node console.run(
                command: "retry"
            )
        }
        else -> {
            fail "bad"
        }
    }

    parallel {
        branch {
            node console.run(
                command: "a"
            )
        }
        branch {
            node console.run(
                command: "b"
            )
        }
    } join any

    try {
        node console.run(
            command: "risky"
        )
    } catch {
        node console.run(
            command: "recover"
        )
    } finally {
        node console.run(
            command: "done"
        )
    }

    race winner first_success {
        branch {
            node console.run(
                command: "primary"
            )
        }
        branch {
            node console.run(
                command: "backup"
            )
        }
    }

    map item in first.items concurrency 2 {
        node console.run(
            command: string(item)
        )
    }

    node cleanup = console.run(
        command: "cleanup"
    )

    node jira.transition(
        base_url: params.jira.base_url,
        email: params.jira.email,
        key: first.output,
        token: "secret",
        transition_id: params.transitions.in_progress
    ).timeout(30s)
}
"#;

    assert_eq!(formatted, expected);
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
    let first = compile(src);
    let second = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition)
    );
}

#[test]
fn format_parenthesizes_eventless_scalar_output() {
    // an event-less scalar payload must keep its parens through formatting, otherwise it would
    // be re-parsed as the event type and silently lose the payload.
    let src = r#"workflow "E" { output ("ready") }"#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("output (\"ready\")"),
        "parens preserved:\n{formatted}"
    );
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);

    let first = compile(src);
    let second = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition)
    );
}

#[test]
fn round_trips_concurrency() {
    let src = r#"
        workflow "Concurrency" v1 {
            node probe = console.run(command: "probe")

            parallel {
                branch { node console.run(command: "lint") }
                branch { node console.run(command: "test") }
            } join all

            race winner first_success {
                branch { node console.run(command: "primary") }
                branch { node console.run(command: "backup") }
            }

            map shard in probe.shards concurrency 4 {
                node console.run(command: "reindex ${shard}")
            }

            try {
                node console.run(command: "risky")
            } catch {
                node console.run(command: "rollback")
            } finally {
                node console.run(command: "cleanup")
            }

            node report = console.run(command: "report")
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_sdlc() {
    let src = r#"
        workflow "Core Team SDLC Pipeline" v1 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
            }
            node tickets = jira.search(jql: params.jira.jql).timeout(120s).retry(3)
            for ticket in tickets.issues limit 50 {
                node spawn "Ticket Work" reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent_workflow_run_id: run.run_id }
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn workflow_namespace_and_qualified_subflow_round_trip() {
    // a `namespace` header rides in metadata, and a qualified subflow target keeps its dotted name.
    let src = r#"
        workflow "Caller" v1 {
            namespace core_sdlc
            node call "core_sdlc.ticket_work" with { id: params.id }
        }
    "#;
    let definition = compile(src);
    assert_eq!(definition.namespace.as_deref(), Some("core_sdlc"));
    let graph = graph_value(&definition);
    let subflow = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "subflow")
        .expect("subflow node");
    assert_eq!(subflow["subflow"]["workflow_name"], "core_sdlc.ticket_work");
    assert_round_trips(src);
}

#[test]
fn import_std_brings_intrinsics_into_bare_scope() {
    // `import std` opens the whole standard library so prefix calls need no qualification; the
    // decompiler still canonicalizes to the qualified form, so the round trip is stable.
    let src = r#"
        workflow "Imp" v1 {
            import std
            node compute {
                let total = add(params.a, params.b)
                return upper(params.name)
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let program = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["program"]
        .to_string();
    // the compiled program holds bare runtime leaves, never the std prefix.
    assert!(program.contains("\"add\""), "program: {program}");
    assert!(
        !program.contains("std.math"),
        "program leaked namespace: {program}"
    );
    assert_round_trips(src);
}

#[test]
fn aliased_module_import_resolves() {
    // `import std.strings as s` binds `s.upper(x)` to the strings module.
    let src = r#"
        workflow "Alias" v1 {
            import std.strings as s
            node slack.send_message(text: s.upper(params.name))
        }
    "#;
    let definition = compile(src);
    let config = graph_value(&definition)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(config.contains("\"$call\":\"upper\""), "config: {config}");
    assert_round_trips(src);
}

#[test]
fn namespaced_provider_action_round_trips() {
    // a dotted provider path keeps every leading segment as the provider; the trailing segment is
    // the function.
    let src = r#"
        workflow "NsAction" v1 {
            node github.repos.create_pr(title: params.title)
        }
    "#;
    let definition = compile(src);
    let action = graph_value(&definition)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]
        .clone();
    assert_eq!(action["provider"], "github.repos");
    assert_eq!(action["function"], "create_pr");
    assert_round_trips(src);
}

#[test]
fn bare_intrinsic_prefix_call_is_rejected() {
    let (_, message) = expect_semantic(
        r#"
        workflow "Bare" v1 {
            node compute { return add(1, 2) }
        }
    "#,
    );
    assert!(message.contains("must be qualified"), "got: {message}");
}

#[test]
fn wrong_std_module_is_rejected() {
    let (_, message) = expect_semantic(
        r#"
        workflow "WrongMod" v1 {
            node compute { return std.math.upper(params.name) }
        }
    "#,
    );
    assert!(
        message.contains("std.strings"),
        "expected a hint to the real module, got: {message}"
    );
}

#[test]
fn for_loop_limit_literal_uses_typed_field() {
    let src = r#"
        workflow "LimitLit" v1 {
            params { items: int[] }
            for n in params.items limit 5 {
                node console.run(command: string(n))
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
fn for_loop_limit_accepts_expression() {
    // an expression cap is carried in the loop parameters (resolved at runtime) and
    // round-trips back to `limit <expr>` through the decompiler.
    let src = r#"
        workflow "LimitExpr" v1 {
            params { items: int[], budget: int }
            for n in params.items limit params.budget {
                node console.run(command: string(n))
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
fn typed_compute_output_hint_validates_loop_items() {
    let src = r#"
        workflow "TypedComputeLoop" v1 {
            node impact: { lambdas: string[] } = compute {
                return { lambdas: ["one", "two"] }
            }
            for lambda_path in impact.lambdas limit none {
                node console.run(command: lambda_path)
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
fn compiles_checked_in_sdlc_ticket_workflow() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/sdlc/wdl/ticket-work.wdl");
    let src = fs::read_to_string(&path).expect("read sdlc ticket workflow");
    let definition = compile(&src);
    assert_eq!(definition.name, "Ticket Work");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/wdl/type_hints/impact/fields/lambdas/ty/type")
            .and_then(Value::as_str),
        Some("array")
    );
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/wdl/type_hints/review_state/fields/changes_requested/ty/type")
            .and_then(Value::as_str),
        Some("integer")
    );
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/wdl/type_hints/deploy_state/fields/failed/ty/type")
            .and_then(Value::as_str),
        Some("integer")
    );
}

#[test]
fn compiles_checked_in_sdlc_pipeline_workflow() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packs/sdlc/wdl/core-team-sdlc-pipeline.wdl");
    let src = fs::read_to_string(&path).expect("read sdlc pipeline workflow");
    let definition = compile(&src);
    assert_eq!(definition.name, "Core Team SDLC Pipeline");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/wdl/type_hints/budget/type")
            .and_then(Value::as_str),
        Some("integer")
    );
}

#[test]
fn round_trips_expression_wait() {
    // wait can take a literal duration or an expression that yields seconds.
    let src = r#"
        workflow "DynWait" v1 {
            params { poll: { interval: int } }
            node seed = console.run(command: "seed")
            wait params.poll.interval until "ready"
            node done = console.run(command: "done")
        }
    "#;
    let definition = compile(src);
    let nodes = definition.definition.as_value();
    let wait = nodes
        .get("nodes")
        .and_then(|n| n.as_array())
        .unwrap()
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("wait"))
        .expect("wait node");
    // the dynamic duration lowers to a $ref expression, not an integer.
    assert!(wait.pointer("/wait/seconds/$ref").is_some(), "{wait:#?}");
    assert_round_trips(src);
}

#[test]
fn node_annotations_lower_and_round_trip() {
    let src = r#"
        workflow "Annotations" v1 {
            @lock
            @timeout(45s)
            wait 1s
        }
    "#;
    let definition = compile(src);
    let nodes = definition.definition.as_value();
    let wait = nodes
        .get("nodes")
        .and_then(|n| n.as_array())
        .unwrap()
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("wait"))
        .expect("wait node");
    assert_eq!(wait.get("locked"), Some(&Value::from(true)));
    assert_eq!(wait.get("timeout_seconds"), Some(&Value::from(45)));

    let wdl = decompile(&definition).expect("decompile");
    assert!(wdl.contains("@lock"), "{wdl}");
    assert!(wdl.contains("@timeout(45s)"), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        definition.definition.as_value(),
        second.definition.as_value()
    );
}

#[test]
fn round_trips_hyphenated_provider() {
    // providers like `ai-command` carry an internal hyphen in the call position.
    let src = r#"
        workflow "Hyphen" v1 {
            node run = ai-command.claude_code(prompt: "hi").timeout(60s)
        }
    "#;
    let definition = compile(src);
    let nodes = definition.definition.as_value();
    let action = nodes
        .get("nodes")
        .and_then(|n| n.as_array())
        .unwrap()
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("action"))
        .expect("action node");
    assert_eq!(
        action.pointer("/action/provider").and_then(|v| v.as_str()),
        Some("ai-command")
    );
    assert_round_trips(src);
}

#[test]
fn lowers_config_and_secret_references() {
    let src = r#"
        workflow "Settings" v1 {
            node go = console.run(command: "x", url: config.api.url, token: secret.github.token)
        }
    "#;
    let definition = compile(src);
    let nodes = definition.definition.as_value();
    let action = nodes
        .get("nodes")
        .and_then(|n| n.as_array())
        .unwrap()
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("action"))
        .expect("action node");
    // config lowers to an eager `$ref` resolved in the web service.
    assert_eq!(
        action
            .pointer("/action/configuration/url/$ref/config/0")
            .and_then(|v| v.as_str()),
        Some("api"),
        "{action:#?}"
    );
    assert_eq!(
        action
            .pointer("/action/configuration/url/$ref/config/1")
            .and_then(|v| v.as_str()),
        Some("url")
    );
    // secret lowers to the late-resolved `secret://scope/name` string form.
    assert_eq!(
        action
            .pointer("/action/configuration/token")
            .and_then(|v| v.as_str()),
        Some("secret://github/token")
    );
    assert_round_trips(src);
}

#[test]
fn lowers_inline_code_to_string_argument() {
    let src = r#"
        workflow "InlineCode" v1 {
            node go = console.run(command: inline("python", ```
print("hello")
```))
        }
    "#;
    let definition = compile(src);
    assert_eq!(
        action_config_value(&definition, "command").as_str(),
        Some("print(\"hello\")\n")
    );
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("inline(\"python\", ```"), "{formatted}");
    assert!(formatted.contains("print(\"hello\")"), "{formatted}");
}

#[test]
fn lowers_file_include_relative_to_source_dir() {
    let mut dir = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    dir.push(format!("runinator-wdl-include-{unique}"));
    fs::create_dir_all(dir.join("scripts")).expect("mkdir");
    fs::write(dir.join("scripts/job.py"), "print('from file')\n").expect("write include");

    let src = r#"
        workflow "FileInclude" v1 {
            node go = console.run(command: file("scripts/job.py"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with include");
    assert_eq!(
        action_config_value(&definition, "command").as_str(),
        Some("print('from file')\n")
    );

    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn file_include_requires_source_dir() {
    let src = r#"
        workflow "FileInclude" v1 {
            node go = console.run(command: file("scripts/job.py"))
        }
    "#;
    match compile_str(src, &CompileOptions::default()) {
        Err(WdlError::Semantic { message, .. }) => {
            assert!(message.contains("source directory"), "{message}");
        }
        other => panic!("expected source directory error, got {other:?}"),
    }
}

#[test]
fn file_include_cannot_escape_source_dir() {
    let src = r#"
        workflow "FileInclude" v1 {
            node go = console.run(command: file("../job.py"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(std::env::temp_dir()),
        ..CompileOptions::default()
    };
    match compile_str(src, &options) {
        Err(WdlError::Semantic { message, .. }) => {
            assert!(message.contains("relative"), "{message}");
        }
        other => panic!("expected unsafe path error, got {other:?}"),
    }
}

fn dir_fixture(label: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    dir.push(format!("runinator-wdl-{label}-{unique}"));
    fs::create_dir_all(dir.join("scripts/lib")).expect("mkdir");
    fs::write(dir.join("scripts/job.py"), "a").expect("write");
    fs::write(dir.join("scripts/setup.py"), "b").expect("write");
    fs::write(dir.join("scripts/lib/util.py"), "c").expect("write");
    dir
}

fn dir_listing(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().expect("string entry").to_string())
            .collect(),
        other => panic!("expected array listing, got {other:?}"),
    }
}

#[test]
fn dir_include_lists_top_level_by_default() {
    let dir = dir_fixture("dir-top");
    let src = r#"
        workflow "DirInclude" v1 {
            node go = console.run(command: dir("scripts"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with dir");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec!["job.py".to_string(), "setup.py".to_string()]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn dir_include_recurses_with_relative_paths() {
    let dir = dir_fixture("dir-recursive");
    let src = r#"
        workflow "DirInclude" v1 {
            node go = console.run(command: dir("scripts", true))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with recursive dir");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec![
            "job.py".to_string(),
            "lib/util.py".to_string(),
            "setup.py".to_string(),
        ]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn dir_include_depth_cap_stops_descent() {
    let dir = dir_fixture("dir-depth");
    let src = r#"
        workflow "DirInclude" v1 {
            node go = console.run(command: dir("scripts", true, 1))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with depth cap");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec!["job.py".to_string(), "setup.py".to_string()]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn dir_include_requires_source_dir() {
    let src = r#"
        workflow "DirInclude" v1 {
            node go = console.run(command: dir("scripts"))
        }
    "#;
    match compile_str(src, &CompileOptions::default()) {
        Err(WdlError::Semantic { message, .. }) => {
            assert!(message.contains("source directory"), "{message}");
        }
        other => panic!("expected source directory error, got {other:?}"),
    }
}

#[test]
fn dir_include_round_trips_through_formatter() {
    let src = r#"workflow "DirInclude" v1 {
    node go = console.run(command: dir("scripts", true, 2))
}
"#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("dir(\"scripts\", true, 2)"),
        "{formatted}"
    );
}

#[test]
fn comparison_operators_lower_to_intrinsic_calls() {
    let src = r#"
        workflow "Cmp" v1 {
            node go = console.run(le: params.x <= 1, eq: params.y == params.z, gt: params.a > 2)
        }
    "#;
    let definition = compile(src);
    for (key, intrinsic) in [("le", "lte"), ("eq", "eq"), ("gt", "gt")] {
        let value = action_config_value(&definition, key);
        let call = value
            .get("$call")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{key} is not a $call: {value:?}"));
        assert_eq!(call, intrinsic, "{key} lowered to wrong intrinsic");
        let args = value.get("args").and_then(Value::as_array).expect("args");
        assert_eq!(args.len(), 2, "{key} should have two operands");
    }
}

#[test]
fn ternary_lowers_to_if_form() {
    let src = r#"
        workflow "Tern" v1 {
            node go = console.run(size: params.n <= 1 ? "small" : "big")
        }
    "#;
    let definition = compile(src);
    let value = action_config_value(&definition, "size");
    assert_eq!(
        value.get("then").and_then(Value::as_str),
        Some("small"),
        "{value:?}"
    );
    assert_eq!(value.get("else").and_then(Value::as_str), Some("big"));
    let cond = value.get("$if").expect("$if branch");
    assert_eq!(cond.get("$call").and_then(Value::as_str), Some("lte"));
}

#[test]
fn ternary_round_trips_through_formatter() {
    let src = "workflow \"Tern\" v1 {\n    node go = console.run(size: params.n <= 1 ? \"small\" : \"big\")\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("params.n <= 1 ? \"small\" : \"big\""),
        "{formatted}"
    );
}

#[test]
fn comparison_round_trips_through_formatter() {
    let src = "workflow \"Cmp\" v1 {\n    node go = console.run(flag: params.x >= 2)\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("params.x >= 2"), "{formatted}");
}

#[test]
fn secret_reference_requires_scope_and_name() {
    let src = r#"
        workflow "BadSecret" v1 {
            node go = console.run(command: "x", token: secret.github)
        }
    "#;
    match compile_str(src, &CompileOptions::default()) {
        Err(WdlError::Lower(message)) => {
            assert!(message.contains("secret"), "{message}")
        }
        other => panic!("expected lower error, got {other:?}"),
    }
}

#[test]
fn round_trips_fanin_error_handlers_and_convergence() {
    // mirrors the Ticket Work shape: linear steps with `fail ->` handlers, a poll loop, an
    // if/approval branch, and several handlers converging on a shared cleanup node. exercises
    // the decompiler's worklist + back-arrow handling for arbitrary fan-in.
    let src = r#"
        workflow "Fanin" v1 {
            params { poll: { interval: integer } }
            node prepare = console.run(command: "prepare")
                fail -> notify_failure
            node build = console.run(command: "build")
                fail -> notify_failure

            until check.status == "passed" || check.status == "failed" limit 20 {
                wait params.poll.interval
                node check = console.run(command: "poll")
            }

            if check.status == "passed" {
                approve "ship it?" type "merge"
                    ok -> finalize
                    reject -> rollback
            } -> notify_failure

            node finalize = console.run(command: "finalize")
                fail -> notify_failure
            node report = console.run(command: "report")
                -> cleanup

            node rollback = console.run(command: "rollback")
                -> cleanup
            node notify_failure = console.run(command: "alert")
                -> cleanup
            node cleanup = console.run(command: "cleanup")
                -> done
        }
    "#;
    assert_round_trips_unordered(src);
}

#[test]
fn round_trips_while_loop() {
    let src = r#"
        workflow "Polling" v1 {
            node seed = console.run(command: "seed")
            while seed.status == "pending" limit 30 {
                node console.run(command: "poll")
            }
            node done = console.run(command: "done")
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
            node seed = console.run(command: "seed")
            until seed.ready == true limit 10 {
                node console.run(command: "poll")
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
            node seed = console.run(command: "seed")
            until seed.ready == true limit 12 {
                node console.run(command: "poll")
            }
            node finish = console.run(command: "finish")
        }
    "#;
    // `until c` round-trips through its negated `while !c` form (graph-equivalent).
    assert_round_trips(src);
}

#[test]
fn round_trips_conditionals() {
    let src = r#"
        workflow "Conditionals" v1 {
            node probe = console.run(command: "probe")
            if probe.count > 0 {
                node console.run(command: "many")
            } else {
                node console.run(command: "none")
            }
            match probe.mode {
                "fast" -> { node console.run(command: "fast") }
                else -> { node console.run(command: "slow") }
            }
            node report = console.run(command: "report")
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_truthy_conditions() {
    let src = r#"
        workflow "TruthyConditions" v1 {
            if true {
                node console.run(command: "yes")
            } else {
                node console.run(command: "no")
            }
            while 1 + 1 limit 1 {
                node console.run(command: "loop")
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_truthy_compute_conditions() {
    let bool_src = r#"
        workflow "TruthyComputeConditions" v1 {
            node compute {
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
            node compute {
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
            node probe = console.run(command: "probe")
            wait 30s until "ready"
            output "checked" { count: probe.count }
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
            node probe = console.run(command: "probe")
            output "count" probe.count
            output "nums" [1, 2, 3]
            output ("ready")
            output (42)
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

    let mut def = compile(r#"workflow "Params" { node console.run(command: "probe") }"#);
    let action = def
        .definition
        .nodes
        .iter_mut()
        .find(|node| node.kind == WorkflowNodeKind::Action)
        .expect("action node");
    action.parameters =
        WorkflowObject::from_value(Value::from(serde_json::json!({ "retries": 3 })))
            .expect("parameters");

    let wdl = decompile(&def).expect("decompile");
    assert!(
        wdl.contains("command:"),
        "configuration arg preserved:\n{wdl}"
    );
    assert!(
        wdl.contains("retries: 3"),
        "node parameter surfaced:\n{wdl}"
    );

    // the surfaced parameter recompiles into the action configuration (same merge result).
    let recompiled = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
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
                node probe = console.run(command: "probe")
                match probe.mode {
                    "fast" -> { node console.run(command: "fast") }
                    else -> { node console.run(command: "slow") }
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
    let wdl = decompile(&not_equals).expect("decompile not_equals shorthand");
    assert!(
        wdl.contains("when") && wdl.contains("!="),
        "not_equals rendered as guard:\n{wdl}"
    );
    compile_str(&wdl, &CompileOptions::default()).expect("recompile not_equals shorthand");

    let exists = rebuild(serde_json::json!({ "exists": true }));
    let wdl = decompile(&exists).expect("decompile exists shorthand");
    assert!(wdl.contains("exists"), "exists rendered as guard:\n{wdl}");
    compile_str(&wdl, &CompileOptions::default()).expect("recompile exists shorthand");
}

#[test]
fn compiles_and_validates_sdlc() {
    let src = r#"
        workflow "Core Team SDLC Pipeline" v1 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
            }

            node tickets = jira.search(
                base_url: params.jira.base_url,
                email:    params.jira.email,
                token:    params.jira.token,
                jql:      params.jira.jql,
            ).timeout(60s)

            for ticket in tickets.issues limit 50 {
                node spawn "Ticket Work" reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent_workflow_run_id: run.run_id }
            }
            -> done
        }
    "#;
    let definition = compile(src);
    assert_eq!(definition.name, "Core Team SDLC Pipeline");

    let graph = definition.definition.as_value();
    let nodes = graph.get("nodes").and_then(|n| n.as_array()).unwrap();
    // find the loop node and check it references the action output for items.
    let loop_node = nodes
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("loop"))
        .expect("loop node");
    let items = loop_node.pointer("/parameters/items").unwrap();
    assert_eq!(
        items.pointer("/$ref/node").and_then(|v| v.as_str()),
        Some("tickets")
    );
    assert_eq!(
        items.pointer("/$ref/output/0").and_then(|v| v.as_str()),
        Some("issues")
    );

    // the subflow run_name should be a $concat with the loop item key.
    let subflow = nodes
        .iter()
        .find(|n| n.get("kind").and_then(|k| k.as_str()) == Some("subflow"))
        .expect("subflow node");
    let run_name = subflow.pointer("/subflow/run_name/$concat").unwrap();
    assert!(run_name.as_array().is_some());
    let ticket_ref = run_name.pointer("/1/$ref/node").and_then(|v| v.as_str());
    assert_eq!(
        ticket_ref,
        subflow
            .get("id")
            .and_then(|v| v.as_str())
            .map(|_| ticket_ref.unwrap())
    );
}

#[test]
fn compiles_control_flow() {
    let src = r#"
        workflow "Control" {
            node probe = console.run(command: "probe")
            if probe.count > 0 && probe.label contains "P0" {
                node console.run(command: "page")
            } else {
                output "skip" { }
            }

            match probe.mode {
                "fast" -> { node console.run(command: "fast") }
                else -> { node console.run(command: "slow") }
            }

            parallel {
                branch { node console.run(command: "a") }
                branch { node console.run(command: "b") }
            } join all

            try {
                node console.run(command: "risky")
            } catch {
                node console.run(command: "recover")
            }
        }
    "#;
    let definition = compile(src);
    let graph = definition.definition.as_value();
    let kinds: Vec<&str> = graph
        .get("nodes")
        .and_then(|n| n.as_array())
        .unwrap()
        .iter()
        .filter_map(|n| n.get("kind").and_then(|k| k.as_str()))
        .collect();
    for expected in [
        "start",
        "condition",
        "switch",
        "parallel",
        "join",
        "try",
        "end",
        "fail",
    ] {
        assert!(kinds.contains(&expected), "missing {expected} node");
    }
}

// semantic analysis -----------------------------------------------------------

#[test]
fn rejects_unknown_reference() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            node console.run(command: ghost.value)
        }
    "#,
    );
    assert!(message.contains("unknown reference 'ghost'"), "{message}");
}

#[test]
fn rejects_unknown_transition_target() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            node console.run(command: "x") -> ghost
        }
    "#,
    );
    assert!(message.contains("unknown step 'ghost'"), "{message}");
}

#[test]
fn rejects_unknown_input_field() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { a: string }
            node console.run(command: params.b)
        }
    "#,
    );
    assert!(message.contains("unknown field 'b'"), "{message}");
}

#[test]
fn rejects_non_array_for_source() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { n: integer }
            for x in params.n { node console.run(command: "y") }
        }
    "#,
    );
    assert!(message.contains("expects an array"), "{message}");
}

#[test]
fn rejects_unorderable_comparison() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { flag: boolean }
            if params.flag > 0 { node console.run(command: "y") }
        }
    "#,
    );
    assert!(message.contains("cannot order"), "{message}");
}

#[test]
fn rejects_loop_var_out_of_scope() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { items: string[] }
            for x in params.items { node console.run(command: "in") }
            node console.run(command: x)
        }
    "#,
    );
    assert!(message.contains("unknown reference 'x'"), "{message}");
}

#[test]
fn rejects_duplicate_node_id() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            node foo = console.run(command: "a")
            node foo = console.run(command: "b")
        }
    "#,
    );
    assert!(message.contains("duplicate node id 'foo'"), "{message}");
}

#[test]
fn warns_on_unreachable_after_fail() {
    let src = r#"
        workflow "Dead" v1 {
            node console.run(command: "ok")
            fail "boom"
            node console.run(command: "never")
        }
    "#;
    let (_, warnings) =
        compile_str_with_diagnostics(src, &CompileOptions::default()).expect("compile");
    assert!(
        warnings.iter().any(|w| w.message.contains("unreachable")),
        "expected unreachable warning, got {warnings:?}"
    );
}

#[test]
fn compute_pure_lowers_to_std_run() {
    let src = r#"
        workflow "Compute" v1 {
            node compute {
                let total = prev.cart.subtotal + prev.cart.tax
                if total <= 0 { goto fail }
                return { total: total }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "run");
    assert!(node["action"]["configuration"]["program"].is_array());
    assert_round_trips(src);
}

#[test]
fn compute_lambda_map_lowers_and_round_trips() {
    let src = r#"
        workflow "Map" v1 {
            node compute {
                let names = std.collections.map(params.users, u => u.name)
                return { names: names }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    // a higher-order call with a pure body stays pure (`std.run`).
    assert_eq!(node["action"]["function"], "run");
    let program = node["action"]["configuration"]["program"].to_string();
    assert!(program.contains("$lambda"), "program: {program}");
    assert!(program.contains("\"map\""), "program: {program}");
    assert_round_trips(src);
}

#[test]
fn compute_lambda_filter_reduce_round_trip() {
    // filter/reduce drive predicates and folds through expression-level intrinsics (gt/add).
    let src = r#"
        workflow "Pipe" v1 {
            node compute {
                let big = std.collections.filter(params.xs, x => std.logic.gt(x, 1))
                let total = std.collections.reduce(big, 0, (acc, x) => std.math.add(acc, x))
                return { total: total }
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn function_defaults_and_lambdas_lower_into_metadata() {
    let src = r#"
        fn fold_values(xs: integer[], seed: integer = 0) -> integer = std.collections.reduce(xs, seed, (acc, x) => std.math.add(acc, x))

        workflow "Fn" v1 {
            node compute {
                let total = fold_values(params.xs)
                return total
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "fold_values");
    assert_eq!(functions[0]["params"][0]["name"], "xs");
    assert_eq!(functions[0]["params"][1]["name"], "seed");
    assert_eq!(functions[0]["body"]["$call"], "reduce");
    assert_eq!(
        functions[0]["body"]["args"][0],
        serde_json::json!({ "$ref": { "let": ["xs"] } })
    );
    assert_eq!(
        functions[0]["body"]["args"][1],
        serde_json::json!({ "$ref": { "let": ["seed"] } })
    );
    assert_eq!(
        functions[0]["body"]["args"][2]["$lambda"]["params"],
        serde_json::json!(["acc", "x"])
    );
    assert_eq!(
        functions[0]["body"]["args"][2]["$lambda"]["body"],
        serde_json::json!({
            "$call": "add",
            "args": [
                { "$ref": { "let": ["acc"] } },
                { "$ref": { "let": ["x"] } }
            ]
        })
    );

    let node = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "run");
    assert_eq!(
        node["action"]["configuration"]["program"][0]["value"],
        serde_json::json!({
            "$call": "fold_values",
            "args": [
                { "$ref": { "params": ["xs"] } },
                0
            ]
        })
    );
}

#[test]
fn pure_block_body_function_lowers_to_program_and_round_trips() {
    let src = r#"
        fn build(a: integer, b: integer) -> integer = {
            let sum = std.math.add(a, b)
            return sum
        }

        workflow "Fn" v1 {
            node compute {
                let total = build(params.x, params.y)
                return total
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert_eq!(functions[0]["name"], "build");
    // a block body lowers to a `program` array, not a single `body` expression.
    assert!(functions[0]["program"].is_array(), "expected program body");
    assert!(functions[0]["body"].is_null(), "expected no expr body");
    // the surface signature is recorded for decompile.
    assert_eq!(
        graph["metadata"]["wdl"]["functions"]["build"],
        "(a: integer, b: integer) -> integer"
    );
    // the caller is pure, so the compute block stays in-process (`std.run`).
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "run");
    assert_round_trips(src);
}

#[test]
fn effectful_block_body_function_forces_caller_to_exec_and_round_trips() {
    let src = r#"
        fn fetch(url: string) -> object = {
            let resp = std.exec.http_get(url)
            return resp.body
        }

        workflow "Fetch" v1 {
            node compute {
                let data = fetch(params.url)
                return data
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert!(functions[0]["program"].is_array(), "expected program body");
    // calling an effectful function makes the enclosing compute block dispatch to the worker.
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(src);
}

#[test]
fn effectful_function_rejected_in_declarative_position() {
    // an effectful function may only be called inside a compute block, never in an action argument.
    let src = r#"
        fn fetch(url: string) -> object = {
            let resp = std.exec.http_get(url)
            return resp.body
        }

        workflow "F" v1 {
            node slack.send_message(text: fetch(params.url))
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
    assert!(message.contains("fetch"), "got: {message}");
    assert!(message.contains("compute block"), "got: {message}");
}

#[test]
fn goto_in_function_body_is_rejected() {
    let src = r#"
        fn bad(x: integer) -> integer = {
            goto somewhere
            return x
        }

        workflow "F" v1 {
            node console.run(command: "x")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("goto"), "got: {message}");
    assert!(message.contains("function body"), "got: {message}");
}

#[test]
fn block_body_function_surface_round_trips_through_formatter() {
    let src = r#"
        fn build(a: integer, b: integer) -> integer = {
            let sum = add(a, b)
            return sum
        }

        workflow "Fn" v1 {
            node console.run(command: "go")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("fn build(a: integer, b: integer) -> integer = {"),
        "{formatted}"
    );
    assert!(formatted.contains("let sum = add(a, b)"), "{formatted}");
    assert!(formatted.contains("return sum"), "{formatted}");
    // formatting is idempotent.
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}

#[test]
fn recursive_function_requires_annotation() {
    let src = r#"
        fn loop(n: integer) = loop(n)

        workflow "Fn" v1 {
            node console.run(command: "go")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("recursive"), "got: {message}");
    assert!(message.contains("@recursive"), "got: {message}");
}

#[test]
fn recursive_function_surface_round_trips_through_formatter() {
    let src = r#"
        @recursive(max_depth: 4)
        fn fold(xs: integer[], seed: integer = 0) -> integer = reduce(xs, seed, (acc, x) => add(acc, x))

        workflow "Fn" v1 {
            node console.run(command: "go")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("@recursive(max_depth: 4)"),
        "{formatted}"
    );
    assert!(
        formatted.contains(
            "fn fold(xs: integer[], seed: integer = 0) -> integer = reduce(xs, seed, (acc, x) => add(acc, x))"
        ),
        "{formatted}"
    );
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}

#[test]
fn compute_effectful_lowers_to_std_exec() {
    let src = r#"
        workflow "Fetch" v1 {
            node compute {
                let resp = std.exec.http_get(params.url)
                return { status: resp.status }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(src);
}

#[test]
fn compute_rejects_goto_in_effectful_block() {
    let src = r#"
        workflow "Bad" v1 {
            node compute {
                let resp = std.exec.http_get(params.url)
                if resp.status > 0 { goto fail }
                return resp
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("goto"), "unexpected message: {message}");
}

#[test]
fn compute_rejects_unknown_intrinsic() {
    let src = r#"
        workflow "Typo" v1 {
            node compute { return addd(1, 2) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("unknown function"), "got: {message}");
}

#[test]
fn compute_rejects_bad_arity() {
    let src = r#"
        workflow "Arity" v1 {
            node compute { return std.math.add(1) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("argument"), "got: {message}");
}

#[test]
fn compute_rejects_let_type_mismatch() {
    let src = r#"
        workflow "Mismatch" v1 {
            node compute { let x: integer = "hello" return x }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("integer"), "got: {message}");
}

#[test]
fn compute_rejects_bad_argument_type() {
    let src = r#"
        workflow "BadArg" v1 {
            node compute { return std.math.add("a", 1) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("argument"), "got: {message}");
}

#[test]
fn compute_lambda_uses_collection_item_type_for_field_access() {
    let src = r#"
        workflow "LambdaTypes" v1 {
            params { users: { id: string }[] }
            node compute {
                return std.collections.map(params.users, u => u.missing)
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(
        message.contains("unknown field 'missing'"),
        "got: {message}"
    );
}

#[test]
fn compute_lambda_result_drives_higher_order_return_type() {
    let src = r#"
        workflow "LambdaReturn" v1 {
            params { users: { id: string }[] }
            node compute {
                let ids: integer[] = std.collections.map(params.users, u => u.id)
                return ids
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'ids'"), "got: {message}");
    assert!(message.contains("expects array"), "got: {message}");
}

#[test]
fn compute_predicate_lambda_must_return_boolean() {
    let src = r#"
        workflow "LambdaPredicate" v1 {
            params { users: { id: string }[] }
            node compute {
                return std.collections.filter(params.users, u => u.id)
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("boolean"), "got: {message}");
    assert!(message.contains("string"), "got: {message}");
}

#[test]
fn compute_accepts_well_typed_program() {
    // a correctly typed program with annotations and a call result flows cleanly.
    let src = r#"
        workflow "Typed" v1 {
            params { a: integer, b: integer }
            node compute {
                let sum: number = std.math.add(params.a, params.b)
                return sum
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn compute_secret_reference_forces_exec() {
    let src = r#"
        workflow "Sec" v1 {
            node compute { return secret.api.key }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap();
    // a secret reference can only resolve at the worker, so the block must be exec.
    assert_eq!(node["action"]["function"], "exec");
}

#[test]
fn compute_condition_allows_arithmetic_and_calls() {
    // arithmetic in a pure condition, and a call (which makes the block exec).
    let pure_src = r#"
        workflow "PureCond" v1 {
            node compute {
                let total = params.a + params.b
                if total * 2 > 100 { goto fail }
                return total
            }
        }
    "#;
    assert_round_trips(pure_src);

    let call_src = r#"
        workflow "CallCond" v1 {
            node compute {
                if std.collections.len(params.items) > 0 {
                    return std.exec.http_get(params.url)
                }
                return null
            }
        }
    "#;
    let definition = compile(call_src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap();
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(call_src);
}

#[test]
fn compute_arithmetic_round_trips() {
    // arithmetic and library calls work both as let/return values and inside object/array literals.
    let src = r#"
        workflow "Math" v1 {
            node compute {
                let x = (params.a + params.b) * 2 - params.c
                return { x: x, y: std.math.add(x, 1), zs: [x, x * 2] }
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn declarative_pure_call_lowers_and_round_trips() {
    // a pure library call now works directly in a declarative action argument (no compute block);
    // it lowers to a `$call` and folds eagerly in the reducer.
    let src = r#"
        workflow "Inline" v1 {
            node slack.send_message(text: std.strings.upper(params.name), count: std.collections.len(params.items))
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("action node");
    let params = node["action"]["configuration"].to_string();
    assert!(params.contains("\"$call\""), "params: {params}");
    assert!(params.contains("\"upper\""), "params: {params}");
    assert_round_trips(src);
}

#[test]
fn declarative_higher_order_call_round_trips() {
    // a higher-order call with a lambda is valid in a declarative argument and round-trips.
    let src = r#"
        workflow "Inline" v1 {
            node slack.send_message(ids: std.collections.map(params.users, u => u.id))
        }
    "#;
    let value = serde_json::to_value(&compile(src).definition).unwrap();
    let params = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(params.contains("\"$lambda\""), "params: {params}");
    // the lambda body's `u.id` must resolve to the lambda-local slot, not a node-output ref.
    assert!(
        params.contains("\"let\""),
        "lambda body not local: {params}"
    );
    assert!(
        !params.contains("\"node\""),
        "lambda body leaked node ref: {params}"
    );
    assert_round_trips(src);
}

#[test]
fn declarative_interpolation_allows_calls() {
    // string interpolation shares the one expression grammar, so a call works inside `${...}`.
    let src = r#"
        workflow "Inline" v1 {
            node slack.send_message(text: "hello ${std.strings.upper(params.name)}")
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(params.contains("\"upper\""), "params: {params}");
    assert_round_trips(src);
}

#[test]
fn postfix_access_on_call_lowers_to_at_and_round_trips() {
    // `.key` / `[i]` chaining on a call result lowers to the `at` intrinsic and decompiles back to
    // access syntax (not `at(...)`).
    let src = r#"
        workflow "Chain" v1 {
            node slack.send_message(text: std.strings.upper(std.strings.split(params.csv, ",")[0]))
        }
    "#;
    let value = serde_json::to_value(&compile(src).definition).unwrap();
    let params = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(params.contains("\"at\""), "params: {params}");
    let wdl = decompile(&compile(src)).expect("decompile");
    assert!(wdl.contains("[0]"), "decompiled: {wdl}");
    assert!(!wdl.contains("at("), "decompiled leaked at(): {wdl}");
    assert_round_trips(src);
}

#[test]
fn method_call_desugars_receiver_first() {
    // `recv.method(args)` lowers to `method(recv, args...)`.
    let src = r#"
        workflow "Fluent" v1 {
            node slack.send_message(text: params.name.upper())
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["text"]
        .clone();
    assert_eq!(
        params,
        serde_json::json!({ "$call": "upper", "args": [{ "$ref": { "params": ["name"] } }] })
    );
    assert_round_trips(src);
}

#[test]
fn fluent_chain_reads_left_to_right_and_round_trips() {
    // a multi-stage fluent pipeline nests into receiver-first calls.
    let src = r#"
        workflow "Fluent" v1 {
            node slack.send_message(ids: params.xs.filter(x => std.logic.gt(x, 1)).map(x => std.math.mul(x, 2)))
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["ids"]
        .clone();
    // outermost call is `map`; its first arg is the `filter` call over the parameter ref.
    assert_eq!(params["$call"], "map");
    assert_eq!(params["args"][0]["$call"], "filter");
    assert_eq!(
        params["args"][0]["args"][0],
        serde_json::json!({ "$ref": { "params": ["xs"] } })
    );
    assert_round_trips(src);
}

#[test]
fn method_call_on_call_result_chains() {
    // `a(..).b(..)` — a method call whose receiver is itself a call result.
    let src = r#"
        workflow "Fluent" v1 {
            node slack.send_message(text: std.strings.split(params.csv, ",").join("-"))
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["text"]
        .clone();
    assert_eq!(params["$call"], "join");
    assert_eq!(params["args"][0]["$call"], "split");
    assert_round_trips(src);
}

#[test]
fn method_call_effectful_receiver_in_compute() {
    // a fluent effectful pipeline lives in a compute block (dispatches to a worker).
    let src = r#"
        workflow "Fetch" v1 {
            node compute {
                let host = std.exec.http_get(params.url).body.host
                return { host: host }
            }
        }
    "#;
    let node = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .cloned()
        .unwrap();
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(src);
}

#[test]
fn method_call_effectful_outside_compute_is_rejected() {
    // `url.http_get()` is the effectful `http_get(url)` — rejected in a declarative position.
    let src = r#"
        workflow "Bad" v1 {
            node slack.send_message(text: params.url.http_get())
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
}

#[test]
fn path_field_named_like_method_still_works() {
    // a plain `.field` (no parens) named like a function stays a path field, not a call.
    let src = r#"
        workflow "Fluent" v1 {
            node slack.send_message(text: params.map.value)
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["text"]
        .clone();
    assert_eq!(
        params,
        serde_json::json!({ "$ref": { "params": ["map", "value"] } })
    );
    assert_round_trips(src);
}

#[test]
fn postfix_access_on_path_folds_into_ref() {
    // chaining static keys onto a path stays a single `$ref`, not an `at` call.
    let src = r#"
        workflow "Chain" v1 {
            node slack.send_message(id: params.items[0].name)
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(
        params.contains("[\"items\",0,\"name\"]"),
        "expected folded ref path: {params}"
    );
    assert!(!params.contains("\"$call\""), "should not use at: {params}");
    assert_round_trips(src);
}

#[test]
fn dynamic_index_lowers_to_at() {
    // a non-literal `[expr]` key never folds into a path; it indexes via `at`.
    let src = r#"
        workflow "Chain" v1 {
            node slack.send_message(v: params.items[params.idx])
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(params.contains("\"$call\":\"at\""), "params: {params}");
    assert_round_trips(src);
}

#[test]
fn effectful_postfix_access_in_compute_lowers_to_exec() {
    // `http_get(url).body` is effectful (the call is), so the compute block dispatches to a worker.
    let src = r#"
        workflow "Fetch" v1 {
            node compute {
                let body = std.exec.http_get(params.url).body
                return { body: body }
            }
        }
    "#;
    let value = serde_json::to_value(&compile(src).definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap();
    assert_eq!(node["action"]["function"], "exec");
    assert!(
        node["action"]["configuration"]
            .to_string()
            .contains("\"at\"")
    );
    assert_round_trips(src);
}

#[test]
fn explicit_at_with_literal_key_is_preserved() {
    // an explicit `at(ref, literal)` must NOT be re-sugared to `ref.key` — that would fold into the
    // path on recompile and change the graph. it stays an `at` call through a round trip.
    let src = r#"
        workflow "At" v1 {
            node slack.send_message(v: std.collections.at(params.items, 0))
        }
    "#;
    let params = serde_json::to_value(&compile(src).definition).unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(params.contains("\"$call\":\"at\""), "params: {params}");
    let wdl = decompile(&compile(src)).expect("decompile");
    assert!(wdl.contains("at("), "explicit at not preserved: {wdl}");
    assert_round_trips(src);
}

#[test]
fn effectful_postfix_access_outside_compute_is_rejected() {
    // the effectful call inside an access chain is still rejected in a declarative position.
    let src = r#"
        workflow "Bad" v1 {
            node slack.send_message(text: std.exec.http_get(params.url))
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
}

#[test]
fn declarative_effectful_call_is_rejected() {
    // an effectful intrinsic outside a compute block is a semantic error (purity, not grammar,
    // is the gate): the reducer cannot run side effects in an eager argument.
    let src = r#"
        workflow "Inline" v1 {
            node slack.send_message(at: std.exec.now())
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(
        message.contains("effectful") && message.contains("compute block"),
        "got: {message}"
    );
}

#[test]
fn declarative_effectful_call_in_condition_is_rejected() {
    // the same rule applies to declarative conditions.
    let src = r#"
        workflow "Inline" v1 {
            if std.exec.now() == params.deadline {
                node slack.send_message(text: "ok")
            }
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
}

#[test]
fn round_trips_named_type_decls() {
    let src = r#"
        workflow "Typed" v1 {
            params {
                cart: Cart
            }
            type Cart { subtotal: number, tax: number }
            type Ids = integer[]
            node console.run(command: "go")
        }
    "#;
    assert_round_trips(src);
    let wdl = decompile(&compile(src)).expect("decompile");
    assert!(wdl.contains("type Cart {"), "struct decl missing:\n{wdl}");
    assert!(
        wdl.contains("type Ids = integer[]"),
        "alias decl missing:\n{wdl}"
    );
    // the parameter field references the declared name, not the expanded struct shape.
    assert!(
        wdl.contains("cart: Cart"),
        "named parameter ref missing:\n{wdl}"
    );
    // a struct type renders each field on its own indented line, not collapsed inline.
    assert!(
        wdl.contains("type Cart {\n        subtotal: number\n        tax: number\n    }"),
        "struct decl not rendered multiline:\n{wdl}"
    );
}

#[test]
fn round_trips_named_type_decls_with_aliases() {
    let src = r#"
        workflow "Typed" v1 {
            type Payload = { response: any }
            alias shared = { input: "hello" }
            node probe: Payload = ai-command.execute(command: "echo", ...shared)
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn named_type_preserved_on_let_annotation() {
    let src = r#"
        workflow "Typed" v1 {
            type Cart { subtotal: number, tax: number }
            node probe: Cart = console.run(command: "probe")
            node console.run(command: "after")
        }
    "#;
    assert_round_trips(src);
    let wdl = decompile(&compile(src)).expect("decompile");
    // the node annotation keeps the declared name rather than expanding the struct.
    assert!(
        wdl.contains("node probe: Cart"),
        "named node ref missing:\n{wdl}"
    );
}

#[test]
fn named_type_resolves_in_input() {
    let src = r#"
        workflow "Typed" v1 {
            params { cart: Cart }
            type Cart { subtotal: number, tax: number }
            node console.run(command: "go")
        }
    "#;
    let definition = compile(src);
    // the input `cart` field resolves to the declared closed struct, not Any.
    let cart = definition.input_type.field("cart").expect("cart field");
    assert!(matches!(cart, RuninatorType::Struct { .. }));
}

#[test]
fn rejects_cyclic_type_decls() {
    let src = r#"
        workflow "Cycle" v1 {
            type A = B
            type B = A
            node console.run(command: "go")
        }
    "#;
    assert!(compile_str(src, &CompileOptions::default()).is_err());
}

#[test]
fn round_trips_let_type_annotation() {
    let src = r#"
        workflow "Typed" v1 {
            node probe: { count: integer } = console.run(command: "probe")
            node console.run(command: "after ${probe.count}")
        }
    "#;
    assert_round_trips(src);
    // the declared type survives compile -> decompile and re-appears in the source.
    let wdl = decompile(&compile(src)).expect("decompile");
    assert!(wdl.contains("node probe:"), "annotation missing:\n{wdl}");
}

// expression-granular spans -------------------------------------------------

#[test]
fn semantic_error_span_points_at_subexpression() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }
            node console.run(command: params.b)
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("unknown field 'b'"), "{message}");
    // the span is the path expression, not the whole statement.
    assert_eq!(&src[span.start..span.end], "params.b", "span = {span:?}");
}

#[test]
fn unorderable_comparison_blames_the_operand() {
    let src = r#"
        workflow "Bad" v1 {
            params { flag: boolean }
            if params.flag > 0 { node console.run(command: "y") }
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("cannot order"), "{message}");
    // the left operand is blamed, not the enclosing if statement.
    assert_eq!(&src[span.start..span.end], "params.flag", "span = {span:?}");
}

#[test]
fn unknown_reference_blames_the_path() {
    let src = r#"
        workflow "Bad" v1 {
            node console.run(command: ghost.value)
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("unknown reference 'ghost'"), "{message}");
    assert_eq!(&src[span.start..span.end], "ghost.value", "span = {span:?}");
}

#[test]
fn renders_semantic_error_with_caret() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }
            node console.run(command: params.b)
        }
    "#;
    let err = compile_str(src, &CompileOptions::default()).unwrap_err();
    let rendered = err.render(src);
    assert!(rendered.contains("error:"), "{rendered}");
    assert!(rendered.contains("^"), "{rendered}");
    // `params.b` sits on the fourth line of the raw string literal.
    assert!(rendered.contains("line 4"), "{rendered}");
}

#[test]
fn analyze_source_reports_all_diagnostics() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }
            node console.run(command: params.b)
            node console.run(command: params.c)
        }
    "#;
    let diagnostics = analyze_source(src).expect("parse");
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    // both unknown-field accesses surface, not just the first.
    assert_eq!(errors.len(), 2, "{diagnostics:?}");
    assert!(errors[0].render(src).contains("^"));
}

// parse-time rich errors -----------------------------------------------------

#[test]
fn unknown_modifier_is_a_syntax_error_with_span() {
    let src = r#"
        workflow "Bad" v1 {
            node console.run(command: "x").bogus()
        }
    "#;
    match parse_document(src) {
        Err(WdlError::Syntax { span, message }) => {
            assert!(message.contains("unknown modifier 'bogus'"), "{message}");
            assert!(span.end > span.start, "empty span {span:?}");
        }
        other => panic!("expected syntax error, got {other:?}"),
    }
}

#[test]
fn parses_minimal_workflow() {
    let src = r#"
        workflow "Hello" v1 {
            node console.run(command: "echo hi")
        }
    "#;
    let doc = parse_document(src).expect("parse");
    assert_eq!(doc.workflow.name, "Hello");
    assert_eq!(
        doc.workflow.version,
        Some(runinator_models::semver::SemVer::new(1, 0, 0))
    );
    assert_eq!(doc.workflow.body.len(), 1);
}

#[test]
fn completes_provider_names_at_action_position() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            ji<>
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"jira".to_string()));
    assert!(labels.contains(&"slack".to_string()));
}

#[test]
fn completes_std_modules_and_functions() {
    let modules = completion_labels(
        r#"
        workflow "Complete" v1 {
            node compute { return std.<> }
        }
    "#,
        "<>",
    );
    assert!(
        modules.contains(&"strings".to_string()),
        "modules: {modules:?}"
    );
    assert!(
        modules.contains(&"collections".to_string()),
        "modules: {modules:?}"
    );

    let functions = completion_labels(
        r#"
        workflow "Complete" v1 {
            node compute { return std.strings.<> }
        }
    "#,
        "<>",
    );
    assert!(
        functions.contains(&"upper".to_string()),
        "functions: {functions:?}"
    );
    assert!(
        !functions.contains(&"add".to_string()),
        "math leaked into strings: {functions:?}"
    );
}

#[test]
fn completes_provider_actions_after_dot() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            jira.<>
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"search".to_string()));
    assert!(labels.contains(&"transition".to_string()));
}

#[test]
fn completes_aliased_std_module_leaves() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            import std.strings as s
            node compute { return s.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"upper".to_string()), "labels: {labels:?}");
    assert!(
        !labels.contains(&"add".to_string()),
        "math leaked through strings alias: {labels:?}"
    );
}

#[test]
fn completes_bare_intrinsics_from_unaliased_import() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            import std.strings
            node compute { return up<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"upper".to_string()), "labels: {labels:?}");
    // a module that was not imported stays out of bare scope.
    assert!(
        !labels.contains(&"merge".to_string()),
        "un-imported module leaked into bare scope: {labels:?}"
    );
}

#[test]
fn does_not_complete_unimported_intrinsics_bare() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node compute { return up<> }
        }
    "#,
        "<>",
    );
    assert!(
        !labels.contains(&"upper".to_string()),
        "unqualified intrinsic offered without import: {labels:?}"
    );
}

#[test]
fn completes_user_functions_bare() {
    let labels = completion_labels(
        r#"
        fn shout(text: string) -> string = text
        workflow "Complete" v1 {
            node compute { return sh<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"shout".to_string()), "labels: {labels:?}");
}

#[test]
fn completes_missing_action_arguments() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            node jira.search(base_url: params.base, <>)
        }
        "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            node jira.search(base_url: params.base, <>)
        }
        "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(!labels.contains(&"base_url"));
    assert!(labels.contains(&"token"));
    // token is a required string, so the snippet pre-fills quotes with an editable field inside.
    assert!(response.items.iter().any(|item| item.label == "token"
        && item.is_snippet
        && item.insert_text == "token: \"${}\""));
}

#[test]
fn completes_nested_input_fields() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            params {
                jira: { base_url: string, token: string }
            }
            node jira.search(base_url: params.jira.<>, token: params.jira.token, jql: "x")
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"base_url".to_string()));
    assert!(labels.contains(&"token".to_string()));
}

#[test]
fn completes_provider_result_outputs() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets = jira.search(base_url: "https://jira", token: "t", jql: "x")
            output "tickets" { issues: tickets.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"issues".to_string()));
    assert!(labels.contains(&"total".to_string()));
}

#[test]
fn explicit_binding_type_overrides_provider_results() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets: { custom: string } = jira.search(base_url: "https://jira", token: "t", jql: "x")
            output "tickets" { value: tickets.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"custom".to_string()));
    assert!(!labels.contains(&"issues".to_string()));
}

#[test]
fn completes_loop_variable_fields_from_array_source() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets = jira.search(base_url: "https://jira", token: "t", jql: "x")
            for item in tickets.issues limit 10 {
                output "ticket" { key: item.<> }
            }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"key".to_string()));
    assert!(labels.contains(&"fields".to_string()));
}

#[test]
fn completes_provider_actions_in_incomplete_source() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            jira.<>
    "#,
        "<>",
    );
    assert!(labels.contains(&"search".to_string()));
}

#[test]
fn suppresses_completion_inside_plain_string() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            output "jira.<>"
        }
    "#,
        "<>",
    );
    assert!(labels.is_empty());
}

#[test]
fn completes_run_context_fields() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            output "run" { id: run.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"run_id".to_string()));
    assert!(labels.contains(&"workflow_id".to_string()));
}

fn setting_completion(src: &str, marker: &str) -> WdlCompletionResponse {
    use runinator_models::settings::{SettingKind, SettingSummary};
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    let settings = vec![
        SettingSummary {
            scope: "github".into(),
            name: "token".into(),
            kind: SettingKind::Secret,
        },
        SettingSummary {
            scope: "github".into(),
            name: "base_url".into(),
            kind: SettingKind::Config,
        },
        SettingSummary {
            scope: "slack".into(),
            name: "webhook".into(),
            kind: SettingKind::Secret,
        },
    ];
    complete_source(WdlCompletionRequest {
        source,
        cursor_byte: cursor,
        providers: completion_providers(),
        settings,
    })
}

#[test]
fn completes_secret_scopes() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            output "out" { token: secret.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert!(labels.contains(&"github".to_string()));
    assert!(labels.contains(&"slack".to_string()));
}

#[test]
fn completes_secret_names_within_scope() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            output "out" { token: secret.github.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    // only the secret-kind name in the github scope is suggested, not the config slot.
    assert_eq!(labels, vec!["token".to_string()]);
}

#[test]
fn completes_config_scopes_separately_from_secrets() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            output "out" { url: config.github.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert_eq!(labels, vec!["base_url".to_string()]);
}

#[test]
fn parameter_defaults_use_typed_placeholders() {
    use runinator_models::providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
    };
    let providers = vec![ProviderMetadata {
        name: "demo".into(),
        actions: vec![ActionMetadata::new("run", "demo").with_parameters(vec![
            ParameterMetadata::required("count", RuninatorType::Integer),
            ParameterMetadata::required("flag", RuninatorType::Boolean),
            ParameterMetadata::optional("name", RuninatorType::String).with_default("ada"),
        ])],
        metadata: ProviderRuntimeMetadata::default(),
    }];
    let src = "workflow \"D\" v1 {\n    node demo.run()\n}";
    let cursor = src.find("()").expect("marker") + 1;
    let inserts = complete_source(WdlCompletionRequest {
        source: src.to_string(),
        cursor_byte: cursor,
        providers,
        settings: Vec::new(),
    })
    .items
    .into_iter()
    .map(|item| (item.label, item.insert_text))
    .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        inserts.get("count").map(String::as_str),
        Some("count: ${0}")
    );
    assert_eq!(
        inserts.get("flag").map(String::as_str),
        Some("flag: ${false}")
    );
    // a concrete default becomes a pre-selected literal.
    assert_eq!(
        inserts.get("name").map(String::as_str),
        Some("name: ${\"ada\"}")
    );
}

#[test]
fn parses_kitchen_sink() {
    let src = r#"
        workflow "Kitchen Sink" v2 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
                github?: { token: string }
                shards: string[]
                labels: map<string>
                payload: { kind: string } | null
            }

            node tickets = jira.search(
                base_url: params.jira.base_url,
                jql:      params.jira.jql,
            ).timeout(60s).retry(3).tags("ci", "release").mcp()

            if tickets.count > 0 && params.jira.jql contains "P0" {
                output "found" { count: tickets.count }
            } else if exists github.token {
                node console.run(command: "noop")
            } else {
                wait 30s until "ready"
            }

            for ticket in tickets.issues limit 50 {
                node spawn "Ticket Work" detached reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent: run.run_id }
            }
            -> done

            match params.payload.kind {
                "fanout" -> { node console.run(command: "a") }
                when params.shards contains "x" -> node console.run(command: "b")
                else -> { output "default" { } }
            }

            parallel {
                branch { node console.run(command: "lint") }
                branch { node console.run(command: "test") }
            } join all -> report

            race winner first_success {
                branch { node console.run(command: "primary") }
                branch { node console.run(command: "backup") }
            }

            map shard in params.shards concurrency 4 {
                node console.run(command: "reindex ${shard}")
            }

            try {
                node console.run(command: "risky")
            } catch {
                node console.run(command: "rollback")
            } finally {
                node console.run(command: "cleanup")
            }

            approve "Ship it?" type "change_request" { env: "prod" }
                ok -> deploy
                reject -> abort

            node deploy = console.run(command: "deploy")
                ok -> done
                fail -> abort

            node abort = console.run(command: "abort")
            node report = console.run(command: "report")

            set name = "renamed: ${tickets.count}"
            fail "done with errors"
        }
    "#;
    let doc = parse_document(src).expect("parse kitchen sink");
    assert_eq!(doc.workflow.name, "Kitchen Sink");
    assert!(doc.workflow.input.is_some());
    assert!(doc.workflow.body.len() >= 12);
}

#[test]
fn alias_spread_lowers_like_explicit_args() {
    let aliased = r#"
        workflow "Aliased" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t = jira.transition(...conn, key: "ABC-1")
        }
    "#;
    let explicit = r#"
        workflow "Aliased" v1 {
            node t = jira.transition(base_url: config.jira.base_url, token: secret.jira.token, key: "ABC-1")
        }
    "#;
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit)),
        "a `...alias` spread should lower identically to the explicit argument list"
    );
}

#[test]
fn explicit_arg_overrides_spread() {
    // the explicit `base_url` wins over the alias's `base_url` regardless of source order.
    let aliased = r#"
        workflow "Override" v1 {
            alias conn = { base_url: "from-alias", region: "us" }
            node t = api.call(...conn, base_url: "explicit")
        }
    "#;
    let explicit = r#"
        workflow "Override" v1 {
            node t = api.call(base_url: "explicit", region: "us")
        }
    "#;
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit))
    );
}

#[test]
fn unknown_alias_spread_is_a_semantic_error() {
    let src = r#"
        workflow "Bad" v1 {
            node t = api.call(...missing, key: "x")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("unknown alias"), "{message}");
}

#[test]
fn duplicate_alias_is_a_semantic_error() {
    let src = r#"
        workflow "Dup" v1 {
            alias conn = { a: "1" }
            alias conn = { b: "2" }
            node t = api.call(...conn)
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("duplicate alias"), "{message}");
}

#[test]
fn format_preserves_alias_and_spread() {
    let src = r#"
        workflow "Fmt" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t = jira.transition(...conn, key: "ABC-1")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("alias conn = {"), "{formatted}");
    assert!(formatted.contains("...conn"), "{formatted}");
    // formatting is idempotent and never expands the sugar.
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}

// normalize a definition's graph and drop the render-only `wdl` metadata sidecar (declared
// types, alias declarations, spread recipes), so forms that differ only in resugar hints —
// aliased vs. fully-expanded source — compare equal on their runtime graph.
fn runtime_graph(definition: runinator_models::workflows::WorkflowDefinition) -> Value {
    let mut value = runinator_workflows::normalize_definition(definition.definition).as_value();
    if let Value::Object(root) = &mut value {
        root.remove("metadata");
    }
    value
}

// helper: compile two sources and assert their runtime graphs match (ignoring resugar hints).
fn assert_same_graph(aliased: &str, explicit: &str) {
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit)),
        "aliased form should lower identically to the explicit form"
    );
}

#[test]
fn object_spread_in_subflow_with_matches_explicit() {
    assert_same_graph(
        r#"
        workflow "Sub" v1 {
            alias conn = { base_url: config.a.b, token: secret.c.d }
            node call "Child" with { ...conn, key: "K" }
        }
        "#,
        r#"
        workflow "Sub" v1 {
            node call "Child" with { base_url: config.a.b, token: secret.c.d, key: "K" }
        }
        "#,
    );
}

#[test]
fn object_spread_in_approval_metadata_matches_explicit() {
    assert_same_graph(
        r#"
        workflow "Appr" v1 {
            alias meta = { env: "prod", owner: "team" }
            approve "Ship?" type "change" { ...meta, extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
        r#"
        workflow "Appr" v1 {
            approve "Ship?" type "change" { env: "prod", owner: "team", extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
    );
}

#[test]
fn nested_object_spread_inside_action_arg() {
    assert_same_graph(
        r#"
        workflow "Nest" v1 {
            alias conn = { base_url: config.a.b }
            node t = api.call(config: { ...conn, timeout: 30 })
        }
        "#,
        r#"
        workflow "Nest" v1 {
            node t = api.call(config: { base_url: config.a.b, timeout: 30 })
        }
        "#,
    );
}

#[test]
fn aliases_compose_via_spread() {
    assert_same_graph(
        r#"
        workflow "Compose" v1 {
            alias base = { base_url: config.a.b }
            alias full = { ...base, token: secret.c.d }
            node t = api.call(...full)
        }
        "#,
        r#"
        workflow "Compose" v1 {
            node t = api.call(base_url: config.a.b, token: secret.c.d)
        }
        "#,
    );
}

#[test]
fn alias_cycle_is_a_semantic_error() {
    let src = r#"
        workflow "Cycle" v1 {
            alias a = { ...b }
            alias b = { ...a }
            node t = api.call(...a)
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("references itself"), "{message}");
}

#[test]
fn later_entry_overrides_spread() {
    // `(x: "from-arg", ...conn)` — the spread is last, so conn's x wins (positional last-wins).
    assert_same_graph(
        r#"
        workflow "Last" v1 {
            alias conn = { x: "from-alias" }
            node t = api.call(x: "from-arg", ...conn)
        }
        "#,
        r#"
        workflow "Last" v1 {
            node t = api.call(x: "from-alias")
        }
        "#,
    );
}

// compile -> decompile -> recompile and assert the full normalized graphs (including the `wdl`
// resugar sidecar) match, so the alias declarations and `...alias` spreads round-trip exactly.
// returns the decompiled source for spot-checks on the recovered surface syntax.
fn assert_alias_round_trips(src: &str) -> String {
    let first = compile(src);
    let wdl = decompile(&first).expect("decompile");
    let second = compile_str(&wdl, &CompileOptions::default())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{wdl}"));
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition),
        "alias round trip diverged\n--- decompiled ---\n{wdl}"
    );
    wdl
}

#[test]
fn resugars_action_spread() {
    let wdl = assert_alias_round_trips(
        r#"
        workflow "Act" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t = jira.transition(...conn, key: "ABC-1")
        }
        "#,
    );
    assert!(wdl.contains("alias conn = {"), "{wdl}");
    assert!(wdl.contains("...conn"), "{wdl}");
    assert!(wdl.contains(r#"key: "ABC-1""#), "{wdl}");
}

#[test]
fn resugars_subflow_with_spread() {
    let wdl = assert_alias_round_trips(
        r#"
        workflow "Sub" v1 {
            alias conn = { base_url: config.a.b, token: secret.c.d }
            node call "Child" with { ...conn, key: "K" }
        }
        "#,
    );
    assert!(wdl.contains("alias conn = {"), "{wdl}");
    assert!(wdl.contains("with {"), "{wdl}");
    assert!(wdl.contains("...conn"), "{wdl}");
}

#[test]
fn resugars_approval_metadata_spread() {
    let wdl = assert_alias_round_trips(
        r#"
        workflow "Appr" v1 {
            alias meta = { env: "prod", owner: "team" }
            approve "Ship?" type "change" { ...meta, extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
    );
    assert!(wdl.contains("alias meta = {"), "{wdl}");
    assert!(wdl.contains("...meta"), "{wdl}");
}

#[test]
fn resugars_nested_object_spread() {
    let wdl = assert_alias_round_trips(
        r#"
        workflow "Nest" v1 {
            alias conn = { base_url: config.a.b }
            node t = api.call(config: { ...conn, timeout: 30 })
        }
        "#,
    );
    // the nested object keeps its `...conn` spread; the formatter lays it out one entry per line.
    assert!(wdl.contains("config: {"), "{wdl}");
    assert!(wdl.contains("...conn"), "{wdl}");
}

#[test]
fn resugars_alias_composition() {
    let wdl = assert_alias_round_trips(
        r#"
        workflow "Compose" v1 {
            alias base = { base_url: config.a.b }
            alias full = { ...base, token: secret.c.d }
            node t = api.call(...full)
        }
        "#,
    );
    // the composing alias keeps its `...base` spread in the recovered header (one entry per line).
    assert!(wdl.contains("alias full = {"), "{wdl}");
    assert!(wdl.contains("...base"), "{wdl}");
    assert!(wdl.contains("...full"), "{wdl}");
}

#[test]
fn resugars_override_keeping_authored_order() {
    // spread-first: the explicit override stays after the spread.
    let first = assert_alias_round_trips(
        r#"
        workflow "Over" v1 {
            alias conn = { base_url: "from-alias", region: "us" }
            node t = api.call(...conn, base_url: "explicit")
        }
        "#,
    );
    // arguments now lay out one per line; the override still follows the spread in source order.
    assert!(
        ordered(&first, "...conn", r#"base_url: "explicit""#),
        "{first}"
    );

    // spread-last: the explicit entry stays before the spread (which wins on recompile).
    let second = assert_alias_round_trips(
        r#"
        workflow "Over2" v1 {
            alias conn = { x: "from-alias" }
            node t = api.call(x: "from-arg", ...conn)
        }
        "#,
    );
    assert!(ordered(&second, r#"x: "from-arg""#, "...conn"), "{second}");
}

// parameter defaults --------------------------------------------------------

#[test]
fn input_default_literal_lowers_and_round_trips() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                count: integer = 5
                label: string = "hello"
            }
            node console.run(command: "go ${params.label}")
        }
    "#;
    let def = compile(src);
    let RuninatorType::Struct { fields, .. } = &def.input_type else {
        panic!("expected struct input, got {:?}", def.input_type);
    };
    let count = fields.get("count").expect("count field");
    assert_eq!(count.default, Some(Value::from(5)));
    // a defaulted field is treated as optional.
    assert!(!count.required, "defaulted field should be optional");

    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("count: integer = 5"), "{wdl}");
    assert!(wdl.contains(r#"label: string = "hello""#), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}

#[test]
fn input_default_expression_round_trips() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                base: string = config.api.base_url
                token: string = secret.api.token
                full: string = config.api.base_url ++ "/v1"
            }
            node console.run(command: params.base)
        }
    "#;
    let def = compile(src);
    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("base: string = config.api.base_url"), "{wdl}");
    assert!(wdl.contains("token: string = secret.api.token"), "{wdl}");
    assert!(
        wdl.contains(r#"full: string = config.api.base_url ++ "/v1""#),
        "{wdl}"
    );
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}

#[test]
fn open_input_struct_lowers_and_round_trips() {
    let src = r#"
        workflow "Open" v1 {
            params {
                name: string
                ...: integer
            }
            node console.run(command: params.name)
        }
    "#;
    let def = compile(src);
    let RuninatorType::Struct { additional, .. } = &def.input_type else {
        panic!("expected struct input, got {:?}", def.input_type);
    };
    assert_eq!(
        additional.as_deref(),
        Some(&runinator_models::types::RuninatorType::Integer)
    );

    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("...: integer"), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}

#[test]
fn rejects_input_default_referencing_prev() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { x: string = prev.foo }
            node console.run(command: params.x)
        }
    "#,
    );
    assert!(
        message.contains("parameter default may only reference"),
        "{message}"
    );
}

#[test]
fn apply_input_defaults_fills_missing_fields() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                count: integer = 5
                label: string = "n-" ++ string(params.count)
                provided: string
            }
            node console.run(command: params.label)
        }
    "#;
    let def = compile(src);
    let mut context = Value::from(serde_json::json!({
        "input": { "provided": "yes" },
        "steps": {},
    }));
    runinator_workflows::apply_input_defaults(&mut context, &def.input_type);
    let input = context.get("input").expect("input slot");
    assert_eq!(input.get("count"), Some(&Value::from(5)));
    assert_eq!(input.get("label"), Some(&Value::from("n-5")));
    // a provided value is never overwritten.
    assert_eq!(input.get("provided"), Some(&Value::from("yes")));
}

#[test]
fn apply_input_defaults_synthesizes_input_when_absent() {
    let src = r#"
        workflow "Defaults" v1 {
            params { greeting: string = "hi" }
            node console.run(command: params.greeting)
        }
    "#;
    let def = compile(src);
    let mut context = Value::from(serde_json::json!({ "steps": {} }));
    runinator_workflows::apply_input_defaults(&mut context, &def.input_type);
    assert_eq!(
        context.get("input").and_then(|i| i.get("greeting")),
        Some(&Value::from("hi"))
    );
}

#[test]
fn format_renders_input_defaults() {
    let src = "workflow \"D\" v1 {\nparams {\ncount: integer = 5\nbase: string = config.x\n}\nnode console.run(command: params.base)\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("count: integer = 5"), "{formatted}");
    assert!(formatted.contains("base: string = config.x"), "{formatted}");
    // formatted source still compiles to the same parameter type.
    let a = compile(src);
    let b = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(a.input_type, b.input_type);
}

// .wdls secrets format ------------------------------------------------------

#[test]
fn parses_wdls_secrets_and_config() {
    use crate::parse_secrets_str;
    use runinator_models::settings::SettingKind;

    let src = r#"
        secret jira.token = "abc123"
        secret jira.api.key = "xyz"
        config jira.base_url = "https://acme.atlassian.net"
        config app.retries = 3
        config app.flags = { beta: true, region: "us" }
    "#;
    let bundle = parse_secrets_str(src).expect("parse wdls");
    assert_eq!(bundle.secrets.len(), 5);

    let token = &bundle.secrets[0];
    assert_eq!(token.scope, "jira");
    assert_eq!(token.name, "token");
    assert_eq!(token.kind, SettingKind::Secret);
    assert_eq!(token.value, Value::from("abc123"));

    // multi-segment names join with `/`, matching wdl secret addressing.
    assert_eq!(bundle.secrets[1].name, "api/key");

    let base = &bundle.secrets[2];
    assert_eq!(base.kind, SettingKind::Config);
    assert_eq!(base.value, Value::from("https://acme.atlassian.net"));

    assert_eq!(bundle.secrets[3].value, Value::from(3));

    let flags = &bundle.secrets[4];
    assert_eq!(flags.kind, SettingKind::Config);
    assert_eq!(flags.value.get("beta"), Some(&Value::from(true)));
    assert_eq!(flags.value.get("region"), Some(&Value::from("us")));
}

#[test]
fn rejects_wdls_reference_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("config app.url = config.other.url\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("literals"), "{message}");
}

#[test]
fn rejects_wdls_interpolated_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("secret app.k = \"a-${params.x}\"\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("interpolate"), "{message}");
}

#[test]
fn wdls_round_trips_through_export() {
    use crate::{parse_secrets_str, secrets_to_wdls};
    let src = r#"
        secret jira.token = "abc123"
        config jira.base_url = "https://acme.atlassian.net"
        config app.flags = { beta: true }
        config app.tags = ["x", "y"]
    "#;
    let bundle = parse_secrets_str(src).expect("parse");
    let rendered = secrets_to_wdls(&bundle);
    let reparsed = parse_secrets_str(&rendered).expect("reparse");
    assert_eq!(bundle.secrets, reparsed.secrets, "rendered:\n{rendered}");
}

// header triggers ------------------------------------------------------------

#[test]
fn lowers_cron_triggers_into_metadata() {
    let src = r#"
        workflow "Scheduled" v1 {
            trigger cron "0 9 * * *"
            trigger cron "*/5 * * * *" with { source: "cron" }
            node Console.run(command: "echo hi")
        }
    "#;
    let def = compile(src);
    let triggers = def
        .definition
        .metadata
        .pointer("/triggers")
        .and_then(Value::as_array)
        .expect("triggers in metadata");
    assert_eq!(triggers.len(), 2);
    assert_eq!(triggers[0].get("cron"), Some(&Value::from("0 9 * * *")));
    assert_eq!(triggers[0].get("enabled"), Some(&Value::from(true)));
    assert_eq!(triggers[1].get("cron"), Some(&Value::from("*/5 * * * *")));
    assert_eq!(
        triggers[1].pointer("/parameters/source"),
        Some(&Value::from("cron"))
    );
}

#[test]
fn trigger_options_lower_and_round_trip() {
    let src = r#"
        workflow "Scheduled" v1 {
            trigger cron "0 9 * * *" with { source: "cron" } disabled blackout "2026-01-01T00:00:00Z" to "2026-01-02T00:00:00Z"
            node Console.run(command: "echo hi")
        }
    "#;
    let def = compile(src);
    let trigger = def
        .definition
        .metadata
        .pointer("/triggers/0")
        .expect("trigger in metadata");
    assert_eq!(trigger.get("enabled"), Some(&Value::from(false)));
    assert_eq!(
        trigger.get("blackout_start"),
        Some(&Value::from("2026-01-01T00:00:00Z"))
    );
    assert_eq!(
        trigger.get("blackout_end"),
        Some(&Value::from("2026-01-02T00:00:00Z"))
    );

    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("disabled"), "{wdl}");
    assert!(
        wdl.contains(r#"blackout "2026-01-01T00:00:00Z" to "2026-01-02T00:00:00Z""#),
        "{wdl}"
    );
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/triggers"),
        second.definition.metadata.pointer("/triggers")
    );
}

#[test]
fn round_trips_cron_triggers() {
    let src = r#"
        workflow "Scheduled" v1 {
            trigger cron "0 9 * * *"
            trigger cron "*/5 * * * *" with { source: "cron" }
            node Console.run(command: "echo hi")
        }
    "#;
    let def = compile(src);
    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("trigger cron \"0 9 * * *\""), "{wdl}");
    assert!(wdl.contains("trigger cron \"*/5 * * * *\" with {"), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/triggers"),
        second.definition.metadata.pointer("/triggers"),
        "triggers diverged:\n{wdl}"
    );
}

#[test]
fn rejects_non_literal_trigger_schedule() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            trigger cron params.schedule
            node Console.run(command: "x")
        }
    "#,
    );
    assert!(message.contains("string literal"), "{message}");
}

#[test]
fn lowers_user_function_into_metadata_and_call() {
    let src = r#"
        fn double(x: integer) -> integer = x * 2
        workflow "Fns" v1 {
            node go = console.run(value: double(21))
        }
    "#;
    let def = compile(src);
    let functions = def
        .definition
        .metadata
        .pointer("/functions")
        .and_then(Value::as_array)
        .expect("functions in metadata");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].get("name"), Some(&Value::from("double")));
    let params = functions[0]
        .get("params")
        .and_then(Value::as_array)
        .expect("params");
    assert_eq!(params[0].get("name"), Some(&Value::from("x")));
    // the body lowers to the multiplication over the parameter local.
    assert!(functions[0].get("body").is_some());
    // the call lowers to the shared `$call` shape with the single positional argument.
    let value = action_config_value(&def, "value");
    assert_eq!(value.get("$call").and_then(Value::as_str), Some("double"));
    let args = value.get("args").and_then(Value::as_array).expect("args");
    assert_eq!(args.len(), 1);
}

#[test]
fn named_args_resolve_to_positional_with_defaults() {
    let src = r#"
        fn greet(name: string, excited: boolean = false) -> string = name
        workflow "Named" v1 {
            node go = console.run(value: greet(name: "ada"))
        }
    "#;
    let def = compile(src);
    let value = action_config_value(&def, "value");
    assert_eq!(value.get("$call").and_then(Value::as_str), Some("greet"));
    let args = value.get("args").and_then(Value::as_array).expect("args");
    // the omitted optional is filled from its default, so both parameters are positional.
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Value::from("ada"));
    assert_eq!(args[1], Value::from(false));
}

#[test]
fn rejects_unannotated_recursion() {
    let message = expect_semantic_error(
        r#"
        fn fact(n: integer) -> integer = n <= 1 ? 1 : n * fact(n - 1)
        workflow "Rec" v1 {
            node go = console.run(value: fact(5))
        }
    "#,
    );
    assert!(message.contains("@recursive"), "{message}");
}

#[test]
fn recursive_function_evaluates_under_runtime() {
    // a `@recursive`-annotated factorial compiles, carries its body in metadata, and the runtime
    // function table evaluates it to a terminating value via the lazy `$if` form.
    let src = r#"
        @recursive(max_depth: 100)
        fn fact(n: integer) -> integer = n <= 1 ? 1 : n * fact(n - 1)
        workflow "Rec" v1 {
            node go = console.run(value: "ok")
        }
    "#;
    let def = compile(src);
    let functions = def.definition.metadata.get("functions").expect("functions");
    let table =
        runinator_workflows::FunctionTable::from_metadata(Some(functions)).expect("function table");
    let call = Value::from(serde_json::json!({ "$call": "fact", "args": [5] }));
    let result = runinator_workflows::resolve_value_refs_with_functions(
        &call,
        &Value::from(serde_json::json!({})),
        &table,
    )
    .expect("evaluate");
    assert_eq!(result, Value::from(120));
}

#[test]
fn rejects_function_shadowing_intrinsic() {
    let message = expect_semantic_error(
        r#"
        fn substring(s: string) -> string = s
        workflow "Shadow" v1 {
            node go = console.run(value: "x")
        }
    "#,
    );
    assert!(message.contains("intrinsic"), "{message}");
}

#[test]
fn function_definition_round_trips_through_formatter() {
    let src = "fn double(x: integer) -> integer = x * 2\n\nworkflow \"Fns\" v1 {\n    node go = console.run(value: double(21))\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("fn double(x: integer)"), "{formatted}");
    assert!(formatted.contains("= x * 2"), "{formatted}");
}

#[test]
fn validates_and_evaluates_expression_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "name": "Ada" } }));
    let value = evaluate_fragment(
        r#""hello " ++ params.name"#,
        WdlFragmentKind::Expression,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate expression");

    assert_eq!(value, Value::from("hello Ada"));
}

#[test]
fn validates_and_evaluates_condition_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "count": 3 } }));
    let value = evaluate_fragment(
        "params.count >= 3 && exists params.count",
        WdlFragmentKind::Condition,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate condition");

    assert_eq!(value, Value::from(true));
}

#[test]
fn validates_and_evaluates_compute_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "count": 3 } }));
    let value = evaluate_fragment(
        r#"{ let doubled = params.count * 2 return doubled + 1 }"#,
        WdlFragmentKind::Compute,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate compute");

    assert_eq!(value.get("outcome").and_then(Value::as_str), Some("return"));
    assert_eq!(value.get("value"), Some(&Value::from(7)));
}

#[test]
fn fragment_validation_rejects_wrong_surface() {
    let err = validate_fragment(
        "workflow \"Not a fragment\" {}",
        WdlFragmentKind::Expression,
        &CompileOptions::default(),
    )
    .unwrap_err();

    assert!(err.to_string().contains("expected"), "{err}");
}

/// the editor regenerates the wdl pane via `decompile` on every refresh/save, so `decompile`
/// output must already be in the formatter's canonical shape or a user's `Format` silently
/// reverts. this guards the struct-type-in-params case that originally diverged.
#[test]
fn decompile_output_is_format_idempotent() {
    let samples: &[&str] = &[
        r#"workflow "Core Team SDLC Pipeline" v1 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
            }
            node tickets = jira.search(jql: params.jira.jql).timeout(120s).retry(3)
            for ticket in tickets.issues limit 50 {
                node spawn "Ticket Work" reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent_workflow_run_id: run.run_id }
            }
        }"#,
        r#"workflow "Concurrency" v1 {
            node probe = console.run(command: "probe")
            parallel {
                branch { node console.run(command: "lint") }
                branch { node console.run(command: "test") }
            } join all
            node report = console.run(command: "report")
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
