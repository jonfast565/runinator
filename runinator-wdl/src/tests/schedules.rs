//! schedule and notification policy headers: concurrency, catchup, and notify, with the option
//! combinations each policy rejects.

use super::*;

#[test]
fn lowers_notify_policies_into_metadata() {
    let src = r##"
        workflow "Nightly" v1 {
            notify on failure -> slack "#oncall"
            notify on sla -> email "ops@example.com" after 30m severity critical
            Console.run(command: "echo hi")
        }
    "##;
    let def = compile(src);
    let policies = def
        .definition
        .metadata
        .pointer("/notifications")
        .and_then(Value::as_array)
        .expect("notifications in metadata");
    assert_eq!(policies.len(), 2);
    assert_eq!(policies[0].get("event"), Some(&Value::from("run_failed")));
    assert_eq!(policies[0].get("channel"), Some(&Value::from("slack")));
    assert_eq!(policies[0].get("target"), Some(&Value::from("#oncall")));
    // severity defaults to warning when the source omits it.
    assert_eq!(policies[0].get("severity"), Some(&Value::from("warning")));
    assert_eq!(policies[0].get("threshold_seconds"), None);
    assert_eq!(
        policies[1].get("event"),
        Some(&Value::from("run_sla_breached"))
    );
    assert_eq!(policies[1].get("channel"), Some(&Value::from("email")));
    assert_eq!(
        policies[1].get("threshold_seconds"),
        Some(&Value::from(1800))
    );
    assert_eq!(policies[1].get("severity"), Some(&Value::from("critical")));
}
#[test]
fn round_trips_notify_policies() {
    let src = r##"
        workflow "Nightly" v1 {
            notify on failure -> slack "#oncall"
            notify on retry_exhausted -> app "ui" severity info
            notify on parked -> slack "#oncall" after 2h with { token: "secret://slack/alt" } disabled
            Console.run(command: "echo hi")
        }
    "##;
    let def = compile(src);
    let wdl = decompile(&def).expect("decompile");
    assert!(
        wdl.contains(r##"notify on failure -> slack "#oncall""##),
        "{wdl}"
    );
    assert!(wdl.contains("after 2h"), "{wdl}");
    assert!(wdl.contains("disabled"), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/notifications"),
        second.definition.metadata.pointer("/notifications"),
        "notifications diverged:\n{wdl}"
    );
}
#[test]
fn lowers_concurrency_and_catchup_into_metadata() {
    let src = r##"
        workflow "Nightly" v1 {
            trigger cron "0 * * * *" catchup fire_all max 10
            concurrency 1 on_conflict queue
            Console.run(command: "echo hi")
        }
    "##;
    let def = compile(src);
    assert_eq!(
        def.definition.metadata.pointer("/concurrency"),
        Some(&runinator_models::json!({
            "max_concurrent_runs": 1,
            "on_conflict": "queue"
        }))
    );
    let triggers = def
        .definition
        .metadata
        .pointer("/triggers")
        .and_then(Value::as_array)
        .expect("triggers in metadata");
    assert_eq!(
        triggers[0].get("catchup"),
        Some(&runinator_models::json!({ "policy": "fire_all", "max_slots": 10 }))
    );
}
#[test]
fn concurrency_defaults_to_skip_and_round_trips() {
    // writing a cap at all means the overlap is unwanted, so the bare form is `skip`; the
    // decompiler always names the policy so a reader never has to know that.
    let src = r##"
        workflow "Nightly" v1 {
            trigger cron "0 * * * *" catchup skip grace 5m
            concurrency 2
            Console.run(command: "echo hi")
        }
    "##;
    let def = compile(src);
    assert_eq!(
        def.definition
            .metadata
            .pointer("/concurrency/on_conflict")
            .and_then(Value::as_str),
        Some("skip")
    );
    let wdl = decompile(&def).expect("decompile");
    assert!(wdl.contains("concurrency 2 on_conflict skip"), "{wdl}");
    assert!(wdl.contains("catchup skip grace 5m"), "{wdl}");
    let second = compile_str(&wdl, &CompileOptions::default()).expect("recompile");
    assert_eq!(
        def.definition.metadata.pointer("/concurrency"),
        second.definition.metadata.pointer("/concurrency"),
        "concurrency diverged:\n{wdl}"
    );
    assert_eq!(
        def.definition.metadata.pointer("/triggers"),
        second.definition.metadata.pointer("/triggers"),
        "catchup diverged:\n{wdl}"
    );
    assert_eq!(format_str(&wdl).expect("format"), wdl);
}
#[test]
fn a_catchup_option_must_match_its_policy() {
    // `grace` only means something to `skip` and `max` only to `fire_all`; accepting the mismatch
    // would store a knob the runtime never reads.
    let src = r##"
        workflow "Nightly" v1 {
            trigger cron "0 * * * *" catchup fire_once grace 5m
            Console.run(command: "echo hi")
        }
    "##;
    let err = compile_str(src, &CompileOptions::default()).expect_err("must reject");
    assert!(err.to_string().contains("grace"), "unexpected error: {err}");
}
#[test]
fn concurrency_must_be_at_least_one() {
    // `concurrency 0` reads as "no runs allowed" but would store as the unlimited sentinel, so it
    // is rejected rather than silently meaning the opposite of what it says.
    let src = r##"
        workflow "Nightly" v1 {
            concurrency 0
            Console.run(command: "echo hi")
        }
    "##;
    let err = compile_str(src, &CompileOptions::default()).expect_err("must reject");
    assert!(
        err.to_string().contains("at least 1"),
        "unexpected error: {err}"
    );
}
#[test]
fn a_duration_notify_event_requires_a_threshold() {
    // `sla`/`parked` are evaluated by a periodic scan; without `after` they could never fire, so
    // this is rejected at compile time rather than importing a policy that is silently inert.
    let src = r##"
        workflow "Nightly" v1 {
            notify on sla -> slack "#oncall"
            Console.run(command: "echo hi")
        }
    "##;
    let err = compile_str(src, &CompileOptions::default()).expect_err("must reject");
    assert!(err.to_string().contains("after"), "unexpected error: {err}");
}
