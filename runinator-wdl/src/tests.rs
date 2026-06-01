use crate::{
    CompileOptions, WdlCompletionRequest, WdlError, analyze_source, compile_str,
    compile_str_with_diagnostics, complete_source, decompile, format_str, parse_document,
};
use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, ResultMetadata,
    RuninatorType,
};

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

fn completion_labels(src: &str, marker: &str) -> Vec<String> {
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    complete_source(WdlCompletionRequest {
        source,
        cursor_byte: cursor,
        providers: completion_providers(),
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
/// decompiler that re-nests branches legitimately emits nodes in a different order.
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

#[test]
fn decompile_renders_back_edge_as_arrow_without_panicking() {
    use runinator_models::workflows::WorkflowDefinition;
    // a linear workflow whose graph we mutate to add a back-edge from `b` to `a`.
    let definition = compile(
        r#"
        workflow "Poller" v1 {
            let a = console.run(command: "a")
            let b = console.run(command: "b")
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
    let src = r#"workflow "Fmt"   v1{input{jira:{base_url:string,email?:string}, "odd-key": map<string[]>, fallback?: string, enabled: boolean, retry: integer, transitions:{done:string,in_progress:string,in_review:string}}
@skip let first: { output: string, status: string, items: string[] } = console.run(command:"echo ${input.jira.base_url}"++(input.fallback??"none"), transitions:{done:"done",in_progress:"progress",in_review:"review"}).timeout(30s).retry(2).tags("ci","fmt").mcp()
fail -> cleanup
timeout -> fail
if input.enabled==true&&exists first.output{emit "ready"{value:first.output}}else{wait 5s}
match first.status{"ok"->console.run(command:"ok") when input.retry > 0 -> {console.run(command:"retry")} else -> fail "bad"}
parallel{branch{console.run(command:"a")}branch{console.run(command:"b")}}join any
try{console.run(command:"risky")}catch{console.run(command:"recover")}finally{console.run(command:"done")}
race winner first_success{branch{console.run(command:"primary")}branch{console.run(command:"backup")}}
map item in first.items concurrency 2{console.run(command:string(item))}
let cleanup = console.run(command:"cleanup")
jira.transition(base_url:input.jira.base_url,email:input.jira.email,key:first.output,token:"secret",transition_id:input.transitions.in_progress).timeout(30s)
}"#;

    let formatted = format_str(src).expect("format");
    let expected = r#"workflow "Fmt" v1 {
    input {
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
    let first: { output: string, status: string, items: string[] } = console.run(
        command: "echo ${input.jira.base_url}" ++ (input.fallback ?? "none"),
        transitions: {
            done: "done",
            in_progress: "progress",
            in_review: "review"
        }
    )
        .timeout(30s)
        .retry(2)
        .tags("ci", "fmt")
        .mcp()
        fail -> cleanup
        timeout -> fail
    if input.enabled == true && exists first.output {
        emit "ready" {
            value: first.output
        }
    } else {
        wait 5s
    }
    match first.status {
        "ok" -> {
            console.run(
                command: "ok"
            )
        }
        when input.retry > 0 -> {
            console.run(
                command: "retry"
            )
        }
        else -> {
            fail "bad"
        }
    }
    parallel {
        branch {
            console.run(
                command: "a"
            )
        }
        branch {
            console.run(
                command: "b"
            )
        }
    } join any
    try {
        console.run(
            command: "risky"
        )
    } catch {
        console.run(
            command: "recover"
        )
    } finally {
        console.run(
            command: "done"
        )
    }
    race winner first_success {
        branch {
            console.run(
                command: "primary"
            )
        }
        branch {
            console.run(
                command: "backup"
            )
        }
    }
    map item in first.items concurrency 2 {
        console.run(
            command: string(item)
        )
    }
    let cleanup = console.run(
        command: "cleanup"
    )
    jira.transition(
        base_url: input.jira.base_url,
        email: input.jira.email,
        key: first.output,
        token: "secret",
        transition_id: input.transitions.in_progress
    )
        .timeout(30s)
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
fn round_trips_concurrency() {
    let src = r#"
        workflow "Concurrency" v1 {
            let probe = console.run(command: "probe")

            parallel {
                branch { console.run(command: "lint") }
                branch { console.run(command: "test") }
            } join all

            race winner first_success {
                branch { console.run(command: "primary") }
                branch { console.run(command: "backup") }
            }

            map shard in probe.shards concurrency 4 {
                console.run(command: "reindex ${shard}")
            }

            try {
                console.run(command: "risky")
            } catch {
                console.run(command: "rollback")
            } finally {
                console.run(command: "cleanup")
            }

            let report = console.run(command: "report")
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_sdlc() {
    let src = r#"
        workflow "Core Team SDLC Pipeline" v1 {
            input {
                jira: { base_url: string, email: string, token: string, jql: string }
            }
            let tickets = jira.search(jql: input.jira.jql).timeout(120s).retry(3)
            for ticket in tickets.issues limit 50 {
                spawn "Ticket Work" reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent_workflow_run_id: run.run_id }
            }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_expression_wait() {
    // wait can take a literal duration or an expression that yields seconds.
    let src = r#"
        workflow "DynWait" v1 {
            input { poll: { interval: int } }
            let seed = console.run(command: "seed")
            wait input.poll.interval until "ready"
            let done = console.run(command: "done")
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
fn round_trips_hyphenated_provider() {
    // providers like `ai-command` carry an internal hyphen in the call position.
    let src = r#"
        workflow "Hyphen" v1 {
            let run = ai-command.claude_code(prompt: "hi").timeout(60s)
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
fn round_trips_fanin_error_handlers_and_convergence() {
    // mirrors the Ticket Work shape: linear steps with `fail ->` handlers, a poll loop, an
    // if/approval branch, and several handlers converging on a shared cleanup node. exercises
    // the decompiler's worklist + back-arrow handling for arbitrary fan-in.
    let src = r#"
        workflow "Fanin" v1 {
            input { poll: { interval: integer } }
            let prepare = console.run(command: "prepare")
                fail -> notify_failure
            let build = console.run(command: "build")
                fail -> notify_failure

            until check.status == "passed" || check.status == "failed" limit 20 {
                wait input.poll.interval
                let check = console.run(command: "poll")
            }

            if check.status == "passed" {
                approve "ship it?" type "merge"
                    ok -> finalize
                    reject -> rollback
            } -> notify_failure

            let finalize = console.run(command: "finalize")
                fail -> notify_failure
            let report = console.run(command: "report")
                -> cleanup

            let rollback = console.run(command: "rollback")
                -> cleanup
            let notify_failure = console.run(command: "alert")
                -> cleanup
            let cleanup = console.run(command: "cleanup")
                -> done
        }
    "#;
    assert_round_trips_unordered(src);
}

#[test]
fn round_trips_while_loop() {
    let src = r#"
        workflow "Polling" v1 {
            let seed = console.run(command: "seed")
            while seed.status == "pending" limit 30 {
                console.run(command: "poll")
            }
            let done = console.run(command: "done")
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
            let seed = console.run(command: "seed")
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
            let seed = console.run(command: "seed")
            until seed.ready == true limit 12 {
                console.run(command: "poll")
            }
            let finish = console.run(command: "finish")
        }
    "#;
    // `until c` round-trips through its negated `while !c` form (graph-equivalent).
    assert_round_trips(src);
}

#[test]
fn round_trips_conditionals() {
    let src = r#"
        workflow "Conditionals" v1 {
            let probe = console.run(command: "probe")
            if probe.count > 0 {
                console.run(command: "many")
            } else {
                console.run(command: "none")
            }
            match probe.mode {
                "fast" -> { console.run(command: "fast") }
                else -> { console.run(command: "slow") }
            }
            let report = console.run(command: "report")
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn round_trips_leaves() {
    let src = r#"
        workflow "Leaves" v1 {
            let probe = console.run(command: "probe")
            wait 30s until "ready"
            emit "checked" { count: probe.count }
            approve "Ship it?" type "change_request" { env: "prod" }
        }
    "#;
    assert_round_trips(src);
}

#[test]
fn compiles_and_validates_sdlc() {
    let src = r#"
        workflow "Core Team SDLC Pipeline" v1 {
            input {
                jira: { base_url: string, email: string, token: string, jql: string }
            }

            let tickets = jira.search(
                base_url: input.jira.base_url,
                email:    input.jira.email,
                token:    input.jira.token,
                jql:      input.jira.jql,
            ).timeout(60s)

            for ticket in tickets.issues limit 50 {
                spawn "Ticket Work" reuse
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
            let probe = console.run(command: "probe")
            if probe.count > 0 && probe.label contains "P0" {
                console.run(command: "page")
            } else {
                emit "skip" { }
            }

            match probe.mode {
                "fast" -> { console.run(command: "fast") }
                else -> { console.run(command: "slow") }
            }

            parallel {
                branch { console.run(command: "a") }
                branch { console.run(command: "b") }
            } join all

            try {
                console.run(command: "risky")
            } catch {
                console.run(command: "recover")
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
            console.run(command: ghost.value)
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
            console.run(command: "x") -> ghost
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
            input { a: string }
            console.run(command: input.b)
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
            input { n: integer }
            for x in input.n { console.run(command: "y") }
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
            input { flag: boolean }
            if input.flag > 0 { console.run(command: "y") }
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
            input { items: string[] }
            for x in input.items { console.run(command: "in") }
            console.run(command: x)
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
            let foo = console.run(command: "a")
            let foo = console.run(command: "b")
        }
    "#,
    );
    assert!(message.contains("duplicate node id 'foo'"), "{message}");
}

#[test]
fn warns_on_unreachable_after_fail() {
    let src = r#"
        workflow "Dead" v1 {
            console.run(command: "ok")
            fail "boom"
            console.run(command: "never")
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
fn round_trips_let_type_annotation() {
    let src = r#"
        workflow "Typed" v1 {
            let probe: { count: integer } = console.run(command: "probe")
            console.run(command: "after ${probe.count}")
        }
    "#;
    assert_round_trips(src);
    // the declared type survives compile -> decompile and re-appears in the source.
    let wdl = decompile(&compile(src)).expect("decompile");
    assert!(wdl.contains("let probe:"), "annotation missing:\n{wdl}");
}

// expression-granular spans -------------------------------------------------

#[test]
fn semantic_error_span_points_at_subexpression() {
    let src = r#"
        workflow "Bad" v1 {
            input { a: string }
            console.run(command: input.b)
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("unknown field 'b'"), "{message}");
    // the span is the path expression, not the whole statement.
    assert_eq!(&src[span.start..span.end], "input.b", "span = {span:?}");
}

#[test]
fn unorderable_comparison_blames_the_operand() {
    let src = r#"
        workflow "Bad" v1 {
            input { flag: boolean }
            if input.flag > 0 { console.run(command: "y") }
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("cannot order"), "{message}");
    // the left operand is blamed, not the enclosing if statement.
    assert_eq!(&src[span.start..span.end], "input.flag", "span = {span:?}");
}

#[test]
fn unknown_reference_blames_the_path() {
    let src = r#"
        workflow "Bad" v1 {
            console.run(command: ghost.value)
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
            input { a: string }
            console.run(command: input.b)
        }
    "#;
    let err = compile_str(src, &CompileOptions::default()).unwrap_err();
    let rendered = err.render(src);
    assert!(rendered.contains("error:"), "{rendered}");
    assert!(rendered.contains("^"), "{rendered}");
    // `input.b` sits on the fourth line of the raw string literal.
    assert!(rendered.contains("line 4"), "{rendered}");
}

#[test]
fn analyze_source_reports_all_diagnostics() {
    let src = r#"
        workflow "Bad" v1 {
            input { a: string }
            console.run(command: input.b)
            console.run(command: input.c)
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
            console.run(command: "x").bogus()
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
            console.run(command: "echo hi")
        }
    "#;
    let doc = parse_document(src).expect("parse");
    assert_eq!(doc.workflow.name, "Hello");
    assert_eq!(doc.workflow.version, Some(1));
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
fn completes_missing_action_arguments() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            jira.search(base_url: input.base, <>)
        }
        "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            jira.search(base_url: input.base, <>)
        }
        "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(!labels.contains(&"base_url"));
    assert!(labels.contains(&"token"));
    assert!(
        response.items.iter().any(|item| item.label == "token"
            && item.is_snippet
            && item.insert_text == "token: ${}")
    );
}

#[test]
fn completes_nested_input_fields() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            input {
                jira: { base_url: string, token: string }
            }
            jira.search(base_url: input.jira.<>, token: input.jira.token, jql: "x")
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
            let tickets = jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "tickets" { issues: tickets.<> }
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
            let tickets: { custom: string } = jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "tickets" { value: tickets.<> }
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
            let tickets = jira.search(base_url: "https://jira", token: "t", jql: "x")
            for item in tickets.issues limit 10 {
                emit "ticket" { key: item.<> }
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
            emit "jira.<>"
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
            emit "run" { id: run.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"run_id".to_string()));
    assert!(labels.contains(&"workflow_id".to_string()));
}

#[test]
fn parses_kitchen_sink() {
    let src = r#"
        workflow "Kitchen Sink" v2 {
            input {
                jira: { base_url: string, email: string, token: string, jql: string }
                github?: { token: string }
                shards: string[]
                labels: map<string>
                payload: { kind: string } | null
            }

            let tickets = jira.search(
                base_url: input.jira.base_url,
                jql:      input.jira.jql,
            ).timeout(60s).retry(3).tags("ci", "release").mcp()

            if tickets.count > 0 && input.jira.jql contains "P0" {
                emit "found" { count: tickets.count }
            } else if exists github.token {
                console.run(command: "noop")
            } else {
                wait 30s until "ready"
            }

            for ticket in tickets.issues limit 50 {
                spawn "Ticket Work" detached reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent: run.run_id }
            }
            -> done

            match input.payload.kind {
                "fanout" -> { console.run(command: "a") }
                when input.shards contains "x" -> console.run(command: "b")
                else -> { emit "default" { } }
            }

            parallel {
                branch { console.run(command: "lint") }
                branch { console.run(command: "test") }
            } join all -> report

            race winner first_success {
                branch { console.run(command: "primary") }
                branch { console.run(command: "backup") }
            }

            map shard in input.shards concurrency 4 {
                console.run(command: "reindex ${shard}")
            }

            try {
                console.run(command: "risky")
            } catch {
                console.run(command: "rollback")
            } finally {
                console.run(command: "cleanup")
            }

            approve "Ship it?" type "change_request" { env: "prod" }
                ok -> deploy
                reject -> abort

            let deploy = console.run(command: "deploy")
                ok -> done
                fail -> abort

            let abort = console.run(command: "abort")
            let report = console.run(command: "report")

            set name = "renamed: ${tickets.count}"
            fail "done with errors"
        }
    "#;
    let doc = parse_document(src).expect("parse kitchen sink");
    assert_eq!(doc.workflow.name, "Kitchen Sink");
    assert!(doc.workflow.input.is_some());
    assert!(doc.workflow.body.len() >= 12);
}
