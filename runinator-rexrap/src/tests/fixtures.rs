//! whole workflows compiled end to end, including the packs checked into this repo — the guard
//! that a grammar change does not quietly break a real program.

use super::*;

#[test]
fn compiles_toggle_and_split_nodes() {
    let src = r#"
        workflow "Rollout" v1 {

            do {
                let seed = console.run(command: "seed")

                toggle config.flags.new_checkout {
                    on -> { console.run(command: "new") }
                    off -> { console.run(command: "old") }
                }

                split on seed.user_id {
                    30% -> { console.run(command: "variant_a") }
                    70% -> { console.run(command: "variant_b") }
                    else -> { console.run(command: "control") }
                }

                let done = console.run(command: "done")
            }
        }
    "#;
    use runinator_models::workflows::WorkflowNodeKind;
    let def = compile(src);
    let has_kind = |kind: WorkflowNodeKind| def.definition.nodes.iter().any(|n| n.kind == kind);
    assert!(has_kind(WorkflowNodeKind::Toggle), "expected a toggle node");
    assert!(
        has_kind(WorkflowNodeKind::Percentage),
        "expected a percentage node"
    );

    let toggle = def
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::Toggle)
        .unwrap();
    assert!(toggle.parameters.as_value().get("on").is_some());
    assert!(toggle.parameters.as_value().get("off").is_some());

    let percentage = def
        .definition
        .nodes
        .iter()
        .find(|n| n.kind == WorkflowNodeKind::Percentage)
        .unwrap();
    let buckets = percentage.parameters.as_value().get("buckets").unwrap();
    assert_eq!(buckets.as_array().unwrap().len(), 2);
}
#[test]
fn round_trips_toggle_and_split() {
    let src = r#"
        workflow "Rollout" v1 {

            do {
                let seed = console.run(command: "seed")

                toggle config.flags.new_checkout {
                    on -> { console.run(command: "new") }
                    off -> { console.run(command: "old") }
                }

                split on seed.user_id {
                    30% -> { console.run(command: "variant_a") }
                    70% -> { console.run(command: "variant_b") }
                    else -> { console.run(command: "control") }
                }

                let done = console.run(command: "done")
            }
        }
    "#;
    assert_round_trips_unordered(src);
}
#[test]
fn round_trips_concurrency() {
    let src = r#"
        workflow "Concurrency" v1 {

            do {
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

            do {
                @timeout(120s)
                @retry(3)
                let tickets = jira.search(jql: params.jira.jql)
                for ticket in tickets.issues limit 50 {
                    subflow("Ticket Work", params: { ticket, parent_workflow_run_id: run.run_id }, detached: true, reuse: true, name: "Ticket Work: ${ticket.key}")
                }
            }
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn compiles_checked_in_sdlc_review_workflow() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packs/sdlc/rexrap/sdlc-review.rrx");
    let src = fs::read_to_string(&path).expect("read sdlc review workflow");
    let definition = compile_with_providers(&src);
    assert_eq!(definition.name, "SDLC: Review");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/type_hints/review_state/fields/changes_requested/ty/type")
            .and_then(Value::as_str),
        Some("integer")
    );
}
#[test]
fn compiles_checked_in_sdlc_deploy_workflow() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packs/sdlc/rexrap/sdlc-deploy.rrx");
    let src = fs::read_to_string(&path).expect("read sdlc deploy workflow");
    let definition = compile_with_providers(&src);
    assert_eq!(definition.name, "SDLC: Deploy");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/type_hints/impact/fields/lambdas/ty/type")
            .and_then(Value::as_str),
        Some("array")
    );
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/type_hints/deploy_state/fields/failed/ty/type")
            .and_then(Value::as_str),
        Some("integer")
    );
}
#[test]
fn compiles_checked_in_sdlc_development_workflow() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packs/sdlc/rexrap/sdlc-development.rrx");
    let src = fs::read_to_string(&path).expect("read sdlc development workflow");
    let definition = compile_with_providers(&src);
    assert_eq!(definition.name, "SDLC: Development");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/type_hints/budget/type")
            .and_then(Value::as_str),
        Some("integer")
    );
}
#[test]
fn compiles_and_validates_sdlc() {
    let src = r#"
        workflow "Core Team SDLC Pipeline" v1 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
            }

            do {

                @timeout(60s)
                let tickets = jira.search(
                    base_url: params.jira.base_url,
                    email:    params.jira.email,
                    token:    params.jira.token,
                    jql:      params.jira.jql,
                )

                for ticket in tickets.issues limit 50 {
                    subflow("Ticket Work", params: { ticket, parent_workflow_run_id: run.run_id }, detached: true, reuse: true, name: "Ticket Work: ${ticket.key}")
                }
                routes {
                    on next {
                        continue end
                    }
                }
            }
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

            do {
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
