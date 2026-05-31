use crate::{
    CompileOptions, WdlError, analyze_source, compile_str, compile_str_with_diagnostics, decompile,
    parse_document,
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
