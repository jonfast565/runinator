//! header trigger declarations: cron and chained triggers lowering into metadata and back.

use super::*;

#[test]
fn lowers_cron_triggers_into_metadata() {
    let src = r#"
        workflow "Scheduled" v1 {
            trigger cron "0 9 * * *"
            trigger cron "*/5 * * * *" with { source: "cron" }
            Console.run(command: "echo hi")
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
            Console.run(command: "echo hi")
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

    let rexrap = decompile(&def).expect("decompile");
    assert!(rexrap.contains("disabled"), "{rexrap}");
    assert!(
        rexrap.contains(r#"blackout "2026-01-01T00:00:00Z" to "2026-01-02T00:00:00Z""#),
        "{rexrap}"
    );
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
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
            Console.run(command: "echo hi")
        }
    "#;
    let def = compile(src);
    let rexrap = decompile(&def).expect("decompile");
    assert!(rexrap.contains("trigger cron \"0 9 * * *\""), "{rexrap}");
    assert!(
        rexrap.contains("trigger cron \"*/5 * * * *\" with {"),
        "{rexrap}"
    );
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/triggers"),
        second.definition.metadata.pointer("/triggers"),
        "triggers diverged:\n{rexrap}"
    );
}
#[test]
fn lowers_chained_triggers_into_metadata() {
    let src = r#"
        workflow "Deploy" v1 {
            trigger on_success workflow "Smoke Tests"
            trigger on_failure workflow "Rollback" with { reason: "deploy failed" }
            trigger on_complete workflow "Notify" disabled
            Console.run(command: "echo deploy")
        }
    "#;
    let def = compile(src);
    let triggers = def
        .definition
        .metadata
        .pointer("/triggers")
        .and_then(Value::as_array)
        .expect("triggers in metadata");
    assert_eq!(triggers.len(), 3);
    assert_eq!(triggers[0].get("kind"), Some(&Value::from("chained")));
    assert_eq!(triggers[0].get("on"), Some(&Value::from("success")));
    assert_eq!(
        triggers[0].get("target_workflow"),
        Some(&Value::from("Smoke Tests"))
    );
    assert_eq!(triggers[0].get("enabled"), Some(&Value::from(true)));
    assert_eq!(triggers[1].get("on"), Some(&Value::from("failure")));
    assert_eq!(
        triggers[1].pointer("/parameters/reason"),
        Some(&Value::from("deploy failed"))
    );
    assert_eq!(triggers[2].get("on"), Some(&Value::from("complete")));
    assert_eq!(triggers[2].get("enabled"), Some(&Value::from(false)));
}
#[test]
fn round_trips_chained_triggers() {
    let src = r#"
        workflow "Deploy" v1 {
            trigger on_success workflow "Smoke Tests"
            trigger on_failure workflow "Rollback" with { reason: "deploy failed" } disabled
            Console.run(command: "echo deploy")
        }
    "#;
    let def = compile(src);
    let rexrap = decompile(&def).expect("decompile");
    assert!(
        rexrap.contains(r#"trigger on_success workflow "Smoke Tests""#),
        "{rexrap}"
    );
    assert!(
        rexrap.contains(r#"trigger on_failure workflow "Rollback" with {"#),
        "{rexrap}"
    );
    assert!(rexrap.contains("disabled"), "{rexrap}");
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/triggers"),
        second.definition.metadata.pointer("/triggers"),
        "triggers diverged:\n{rexrap}"
    );
}
#[test]
fn rejects_non_literal_trigger_schedule() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            trigger cron params.schedule
            Console.run(command: "x")
        }
    "#,
    );
    assert!(message.contains("string literal"), "{message}");
}
