//! operators and their surface: comparisons and ternaries lowering to intrinsic calls, and the
//! form they format back to.

use super::*;

#[test]
fn comparison_operators_lower_to_intrinsic_calls() {
    let src = r#"
        workflow "Cmp" v1 {
            node go <- console.run(le: params.x <= 1, eq: params.y == params.z, gt: params.a > 2)
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
            node go <- console.run(size: params.n <= 1 ? "small" : "big")
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
    let src = "workflow \"Tern\" v1 {\n    node go <- console.run(size: params.n <= 1 ? \"small\" : \"big\")\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("params.n <= 1 ? \"small\" : \"big\""),
        "{formatted}"
    );
}
#[test]
fn comparison_round_trips_through_formatter() {
    let src = "workflow \"Cmp\" v1 {\n    node go <- console.run(flag: params.x >= 2)\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("params.x >= 2"), "{formatted}");
}
#[test]
fn secret_reference_requires_scope_and_name() {
    let src = r#"
        workflow "BadSecret" v1 {
            node go <- console.run(command: "x", token: secret.github)
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
            node prepare <- console.run(command: "prepare")
                fail -> notify_failure
            node build <- console.run(command: "build")
                fail -> notify_failure

            until check.status == "passed" || check.status == "failed" limit 20 {
                wait params.poll.interval
                node check <- console.run(command: "poll")
            }

            if check.status == "passed" {
                approve "ship it?" type "merge"
                    ok -> finalize
                    reject -> rollback
            } -> notify_failure

            node finalize <- console.run(command: "finalize")
                fail -> notify_failure
            node report <- console.run(command: "report")
                -> cleanup

            node rollback <- console.run(command: "rollback")
                -> cleanup
            node notify_failure <- console.run(command: "alert")
                -> cleanup
            node cleanup <- console.run(command: "cleanup")
                -> done
        }
    "#;
    assert_round_trips_unordered(src);
}
