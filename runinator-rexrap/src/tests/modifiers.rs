//! clause modifiers on a node: retry, compensation, watch guards, correlation keys, runner
//! labels, idempotency, and the wait-until desugaring.

use super::*;

#[test]
fn explicit_decompile_surfaces_every_implicit_part() {
    // a single action whose terse form hides start, ids, the success edge, and the defaults.
    let rexrap = assert_round_trips_explicit(
        r#"
        workflow "Hello" v1 {
            node greeting <- console.run(command: "echo hi")
        }
    "#,
    );
    assert!(
        rexrap.contains("start -> greeting"),
        "missing start edge:\n{rexrap}"
    );
    assert!(
        rexrap.contains(".timeout(60s)"),
        "missing default timeout:\n{rexrap}"
    );
    assert!(
        rexrap.contains(".retry(1)"),
        "missing default retry:\n{rexrap}"
    );
    assert!(
        rexrap.contains("ok -> done"),
        "missing success arrow:\n{rexrap}"
    );
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

/// a `compensate` clause is the same `action_stmt` as the forward call, so it carries the same
/// modifiers and lowering has always kept them. the decompiler used to render only
/// `provider.function(args)`, which silently reverted a compensation's timeout, tags, and runner
/// every time the graph editor saved through its rexrap round trip.
#[test]
fn compensation_modifiers_survive_the_round_trip() {
    let source = r#"
        workflow "Saga" v1 {
            node deploy <- console.run(command: "deploy")
                compensate console.run(command: "rollback").timeout(300s).tags("undo").runner("ops")
            node verify <- console.run(command: "verify")
        }
    "#;
    let definition = compile(source);
    let compensation = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "deploy")
        .and_then(|node| node.compensation.as_ref())
        .expect("compensation present");
    assert_eq!(compensation.timeout_seconds, 300);
    assert_eq!(compensation.tags, vec!["undo".to_string()]);
    assert_eq!(
        compensation
            .required_labels
            .get("runner")
            .map(String::as_str),
        Some("ops")
    );

    let text = crate::decompile(&definition).expect("decompile");
    assert!(
        text.contains(".timeout(300s)") && text.contains(".tags(\"undo\")"),
        "compensation modifiers missing from the rendered rexrap:\n{text}"
    );
    assert_round_trips_unordered(source);
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
    let rexrap = decompile(&definition).expect("decompile");
    assert!(
        rexrap.contains(".runner(\"creds-sync\")"),
        "decompiled source missing runner modifier:\n{rexrap}"
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

    let rexrap = decompile(&definition).expect("decompile");
    assert!(
        rexrap.contains(".idempotent(key: run.run_id)"),
        "decompiled source missing idempotent modifier:\n{rexrap}"
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

/// a header `interrupt on <source> { ... }` region lowers into `metadata.interrupts` and survives
/// the round trip, including the region's own nodes.
#[test]
fn interrupt_region_lowers_to_metadata_and_round_trips() {
    let definition = compile(
        r#"
        workflow "Interrupt" v1 {
            interrupt on wake {
                node refresh <- console.run(command: "echo refresh")
                resume next
            }

            wait 30s
        }
    "#,
    );
    let interrupts = definition
        .definition
        .metadata
        .pointer("/interrupts")
        .and_then(|value| value.as_array())
        .expect("interrupts metadata");
    assert_eq!(interrupts.len(), 1);
    assert_eq!(
        interrupts[0].get("on").and_then(|on| on.as_str()),
        Some("wake")
    );
    assert_eq!(
        interrupts[0].get("handler").and_then(|h| h.as_str()),
        Some("__interrupt_0_entry"),
        "the region's entry is its own interrupt node, not its first statement"
    );
    let entry = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "__interrupt_0_entry")
        .expect("the entry node is emitted");
    assert_eq!(
        entry.kind,
        runinator_models::workflows::WorkflowNodeKind::Interrupt
    );
    assert!(
        entry.parameters.get("on").is_none(),
        "the source-to-entry link belongs only to metadata"
    );
    assert_eq!(
        entry.transitions.next.as_ref().map(|next| next.as_str()),
        Some("refresh"),
        "and hands straight to the block's first statement"
    );

    assert_round_trips_unordered(
        r#"
        workflow "Interrupt" v1 {
            interrupt on wake {
                node refresh <- console.run(command: "echo refresh")
                resume next
            }

            wait 30s
        }
    "#,
    );
}

#[test]
fn a_disabled_interrupt_link_round_trips_through_rexrap() {
    let source = r#"
        workflow "Disabled interrupt" v1 {
            interrupt on wake disabled {
                audit action "record wake"
                resume
            }
            wait 30s
        }
    "#;
    let definition = compile(source);
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/interrupts/0/enabled")
            .and_then(|value| value.as_bool()),
        Some(false)
    );

    let rendered = decompile(&definition).expect("disabled handler decompiles");
    assert!(rendered.contains("interrupt on wake disabled {"));
    assert_round_trips_unordered(source);
}

/// every source the runtime knows must be spellable, lower to its own name, and round-trip. the
/// grammar lists them as alternatives, so a name that prefixes another (or one simply left out) is
/// the failure this catches — silently, since the parser would just reject the program.
#[test]
fn every_interrupt_source_parses_and_round_trips() {
    for source in runinator_models::interrupt::InterruptSource::ALL {
        let src = format!(
            r#"
            workflow "Sources" v1 {{
                interrupt on {source} {{
                    node refresh <- console.run(command: "echo refresh")
                    resume
                }}

                wait 30s
            }}
        "#
        );
        let definition = compile(&src);
        assert_eq!(
            definition
                .definition
                .metadata
                .pointer("/interrupts/0/on")
                .and_then(|on| on.as_str()),
            Some(source.as_str()),
            "`{source}` must lower to its own name"
        );
        assert_round_trips_unordered(&src);
    }
}

/// every `resume` mode survives the round trip. the compiled form of `resume next` is
/// `mode: "continue"`, so this is also the guard on that one-way spelling.
#[test]
fn every_resume_mode_round_trips() {
    for (source, compiled) in [
        ("resume", "resume"),
        ("resume next", "continue"),
        ("resume restart", "restart"),
        ("resume fail", "fail"),
    ] {
        let src = format!(
            r#"
            workflow "Resume Mode" v1 {{
                interrupt on wake {{
                    node refresh <- console.run(command: "echo refresh")
                    {source}
                }}

                wait 30s
            }}
        "#
        );
        let definition = compile(&src);
        let node = definition
            .definition
            .nodes
            .iter()
            .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Resume)
            .unwrap_or_else(|| panic!("`{source}` produced no resume node"));
        assert_eq!(
            node.parameters.get("mode").and_then(|m| m.as_str()),
            Some(compiled),
            "`{source}` must compile to mode {compiled}"
        );
        assert_round_trips_unordered(&src);
    }
}

/// a region whose block does not end in a `resume` still gets one, so no path can run off the end
/// of a handler and leave the suspended thread with nothing to release it.
#[test]
fn a_region_without_an_explicit_resume_gets_a_synthetic_one() {
    let definition = compile(
        r#"
        workflow "Implicit Resume" v1 {
            interrupt on wake {
                node refresh <- console.run(command: "echo refresh")
            }

            wait 30s
        }
    "#,
    );
    let resumes: Vec<_> = definition
        .definition
        .nodes
        .iter()
        .filter(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Resume)
        .collect();
    assert_eq!(
        resumes.len(),
        1,
        "the lowerer terminates the region for the author"
    );
    let refresh = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id.as_str() == "refresh")
        .expect("region body");
    assert_eq!(
        refresh
            .transitions
            .on_success
            .as_ref()
            .map(|target| target.as_str()),
        Some(resumes[0].id.as_str()),
        "the block's continuation is the synthetic resume"
    );
}

/// a legacy graph editor scaffold points metadata directly at an `audit` body and a bare `resume`.
///
/// it never passes through the rexrap front end on the way in, but saving decompiles it and the server
/// recompiles the text, so the shape has to survive that trip. a region the decompiler cannot render
/// is either rejected at save or silently rewritten, and neither failure is visible in the editor.
#[test]
fn a_scaffolded_interrupt_region_survives_the_save_round_trip() {
    let scaffolded: runinator_models::workflows::WorkflowDefinition = serde_json::from_value(
        serde_json::json!({
            "name": "Scaffolded",
            "version": "1.0.0",
            "enabled": true,
            "definition": {
                "start": "start",
                "nodes": [
                    { "id": "start", "kind": "start", "transitions": { "next": { "$node": "wait_a_bit" } } },
                    { "id": "wait_a_bit", "kind": "wait", "wait": { "seconds": 30 },
                      "transitions": { "next": { "$node": "end" } } },
                    { "id": "end", "kind": "end" },
                    { "id": "on_external", "kind": "audit",
                      "parameters": { "action": "interrupt:external" },
                      "transitions": { "next": { "$node": "resume_external" } } },
                    { "id": "resume_external", "kind": "resume", "parameters": { "mode": "resume" } }
                ],
                "metadata": { "interrupts": [{ "on": "external", "handler": "on_external" }] }
            }
        }),
    )
    .expect("scaffolded definition");

    let source = decompile(&scaffolded).expect("scaffolded region must decompile");
    assert!(
        source.contains("interrupt on external {"),
        "the declaration must reach the rexrap header, got:\n{source}"
    );

    let recompiled = compile_str(&source, &default_test_options()).expect("recompile");
    let interrupts = recompiled
        .definition
        .metadata
        .pointer("/interrupts")
        .and_then(|value| value.as_array())
        .expect("interrupts survive the round trip");
    assert_eq!(interrupts.len(), 1);
    assert_eq!(
        interrupts[0].get("on").and_then(|on| on.as_str()),
        Some("external")
    );
    // the region's nodes come back, and the entry still points at a resume.
    let entry = interrupts[0]
        .get("handler")
        .and_then(|handler| handler.as_str())
        .expect("handler");
    let entry_node = recompiled
        .definition
        .nodes
        .iter()
        .find(|node| node.id == entry)
        .expect("region entry survives");
    // the recompiled region is entered at a real `interrupt` node even though the input declared
    // its handler only in metadata: this is the fallback-to-graph migration, and it is the one
    // end-to-end proof that a definition written before the entry node existed still round-trips.
    assert_eq!(
        entry_node.kind,
        runinator_models::workflows::WorkflowNodeKind::Interrupt
    );
    assert!(entry_node.parameters.get("on").is_none());
    let body = entry_node
        .transitions
        .next
        .as_ref()
        .map(|next| next.as_str().to_string())
        .expect("the entry hands to the region body");
    assert_eq!(
        recompiled
            .definition
            .nodes
            .iter()
            .find(|node| node.id == body)
            .map(|node| node.kind.clone()),
        Some(runinator_models::workflows::WorkflowNodeKind::Audit),
        "the audit node the scaffold created is now the body rather than the entry"
    );
    assert!(
        recompiled
            .definition
            .nodes
            .iter()
            .any(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Resume),
        "the region must still end at a resume"
    );
}

#[test]
fn a_call_site_policy_parses_and_survives_formatting() {
    // the `with { … }` postfix on a call inside a `do` block: per-call overrides of the node policy.
    // this pins the syntax contract — parse and re-render. carrying the policy through lowering
    // into the compiled graph is the invocation-ir lowering's job, not the grammar's.
    let src = r#"
        workflow "Policy" v1 {
            node result <- do {
                return std.strings.upper("hi") with { timeout: 30s }
            }
        }
    "#;
    let formatted = crate::format_str(src).expect("format");
    assert!(
        formatted.contains("with {"),
        "policy postfix lost:\n{formatted}"
    );
    // and it is stable: formatting the formatted source is a fixed point.
    let again = crate::format_str(&formatted).expect("reformat");
    assert_eq!(formatted, again, "formatting is not idempotent");
}

#[test]
fn a_call_site_policy_does_not_swallow_a_cron_triggers_options() {
    // `with` already introduces an object after a trigger's schedule expression. the policy postfix
    // attaches to call syntax only, so a schedule string keeps its own `with`.
    let definition = compile(
        r#"
        workflow "Scheduled" v1 {
            trigger cron "0 * * * *" with { tz: "UTC" }
            node go <- console.run(command: "echo hi")
        }
    "#,
    );
    let triggers = definition
        .definition
        .metadata
        .get("triggers")
        .and_then(runinator_models::value::Value::as_array)
        .expect("triggers");
    assert_eq!(triggers.len(), 1, "trigger lost: {triggers:?}");
    // the `with` object belongs to the trigger, wherever the lowerer files it.
    let rendered = format!("{:?}", triggers[0]);
    assert!(
        rendered.contains("UTC"),
        "the trigger's own `with` object was captured elsewhere: {rendered}"
    );
}

#[test]
fn a_notify_policys_with_object_is_still_its_own() {
    let definition = compile(
        r#"
        workflow "Notified" v1 {
            notify on failure -> slack "ops" with { channel: "alerts" }
            node go <- console.run(command: "echo hi")
        }
    "#,
    );
    let notifications = definition
        .definition
        .metadata
        .get("notifications")
        .and_then(runinator_models::value::Value::as_array)
        .expect("notifications");
    assert_eq!(notifications.len(), 1);
}
