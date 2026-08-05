//! clause modifiers on a node: retry, compensation, watch guards, correlation keys, runner
//! labels, idempotency, and the wait-until desugaring.

use super::*;

#[test]
fn explicit_decompile_surfaces_every_implicit_part() {
    // a single action whose terse form hides start, ids, the success edge, and the defaults.
    let wdl = assert_round_trips_explicit(
        r#"
        workflow "Hello" v1 {
            node greeting <- console.run(command: "echo hi")
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
            node go <- console.run(command: "echo hi")
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
            node deploy <- console.run(command: "deploy")
                compensate console.run(command: "rollback")
            node verify <- console.run(command: "verify")
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
            node deploy <- console.run(command: "deploy")
                compensate console.run(command: "rollback")
            node verify <- console.run(command: "verify")
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
            node work <- console.run(command: "echo work")
            node handle_drift <- console.run(command: "echo drift")
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
            node work <- console.run(command: "echo work")
            node handle_drift <- console.run(command: "echo drift")
        }
    "#,
    );
}
#[test]
fn correlate_header_lowers_to_metadata_and_round_trips() {
    let definition = compile(
        r#"
        workflow "Orders" v1 {
            params { batch_id: string }
            correlate key params.batch_id
            node work <- console.run(command: "echo work")
        }
    "#,
    );
    assert!(
        definition
            .definition
            .metadata
            .pointer("/correlation")
            .is_some(),
        "correlation metadata"
    );

    assert_round_trips_unordered(
        r#"
        workflow "Orders" v1 {
            params { batch_id: string }
            correlate key params.batch_id
            node work <- console.run(command: "echo work")
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
            node seed <- console.run(command: "echo go")
            signal "github.review" key params.ticket.key
            node after <- console.run(command: "echo done")
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
            node seed <- console.run(command: "echo go")
            signal "github.review" key params.ticket.key
            node after <- console.run(command: "echo done")
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
            node seed <- console.run(command: "echo go")
            wait until seed.status == "ready" every 15s
            node after <- console.run(command: "echo done")
        }
    "#,
    );
    let explicit = compile(
        r#"
        workflow "WaitUntil" v1 {
            node seed <- console.run(command: "echo go")
            until seed.status == "ready" {
                wait 15s
            }
            node after <- console.run(command: "echo done")
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
            node seed <- console.run(command: "echo go")
            wait until seed.status == "ready"
            node after <- console.run(command: "echo done")
        }
    "#,
    );
}
#[test]
fn retry_config_round_trips() {
    assert_round_trips(
        r#"
        workflow "Retry" v1 {
            node go <- console.run(command: "echo hi")
                .retry(4, backoff: 2s, max: 60s, jitter: true, on: failure)
        }
    "#,
    );
}
#[test]
fn runner_modifier_lowers_and_round_trips() {
    let src = r#"
        workflow "Runner" v1 {
            node go <- console.run(command: "echo hi")
                .runner("creds-sync")
                .timeout(300s)
        }
    "#;
    let definition = compile(src);
    let action = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .and_then(|node| node.action.as_ref())
        .expect("action node");
    assert_eq!(
        action.required_labels.get("runner").map(String::as_str),
        Some("creds-sync"),
        "runner modifier should lower to required_labels.runner"
    );

    // the decompiled source must surface `.runner("creds-sync")` and round-trip.
    let wdl = decompile(&definition).expect("decompile");
    assert!(
        wdl.contains(".runner(\"creds-sync\")"),
        "decompiled source missing runner modifier:\n{wdl}"
    );
    assert_round_trips(src);
}
#[test]
fn idempotent_modifier_lowers_and_round_trips() {
    // the key is an expression, not a literal: it names *this* run's effect, so it has to be able to
    // read run inputs the way any other action argument does.
    let src = r#"
        workflow "Charges" v1 {
            node charge <- billing.charge(amount: 100)
                .idempotent(key: run.run_id)
        }
    "#;
    let definition = compile(src);
    let action = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .and_then(|node| node.action.as_ref())
        .expect("action node");
    assert!(
        action.idempotency_key.is_some(),
        "idempotent modifier should lower to action.idempotency_key"
    );

    let wdl = decompile(&definition).expect("decompile");
    assert!(
        wdl.contains(".idempotent(key: run.run_id)"),
        "decompiled source missing idempotent modifier:\n{wdl}"
    );
    assert_round_trips(src);
}
#[test]
fn action_without_idempotent_modifier_carries_no_key() {
    // the default has to stay off: a key nobody asked for would silently dedupe real work.
    let src = r#"
        workflow "Plain" v1 {
            node go <- console.run(command: "echo hi")
        }
    "#;
    let definition = compile(src);
    let action = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .and_then(|node| node.action.as_ref())
        .expect("action node");
    assert!(action.idempotency_key.is_none());
}
