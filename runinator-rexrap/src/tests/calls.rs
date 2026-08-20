//! call syntax: declarative calls, method-call desugaring, fluent chains, and postfix access
//! folding into `at`/refs.

use super::*;

#[test]
fn declarative_pure_call_lowers_and_round_trips() {
    // a pure library call now works directly in a declarative action argument (no compute block);
    // it lowers to a `$call` and folds eagerly in the reducer.
    let src = r#"
        workflow "Inline" v1 {
            slack.send_message(text: std.strings.upper(params.name), count: std.collections.len(params.items))
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
            slack.send_message(ids: std.collections.map(params.users, u => u.id))
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
            slack.send_message(text: "hello ${std.strings.upper(params.name)}")
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
            slack.send_message(text: std.strings.upper(std.strings.split(params.csv, ",")[0]))
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
    let rexrap = decompile(&compile(src)).expect("decompile");
    assert!(rexrap.contains("[0]"), "decompiled: {rexrap}");
    assert!(!rexrap.contains("at("), "decompiled leaked at(): {rexrap}");
    assert_round_trips(src);
}
#[test]
fn method_call_desugars_receiver_first() {
    // `recv.method(args)` lowers to `method(recv, args...)`.
    let src = r#"
        workflow "Fluent" v1 {
            slack.send_message(text: params.name.upper())
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
            slack.send_message(ids: params.xs.filter(x => std.logic.gt(x, 1)).map(x => std.math.mul(x, 2)))
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
            slack.send_message(text: std.strings.split(params.csv, ",").join("-"))
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
            do {
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
            slack.send_message(text: params.url.http_get())
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
            slack.send_message(text: params.map.value)
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
            slack.send_message(id: params.items[0].name)
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
            slack.send_message(v: params.items[params.idx])
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
            do {
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
            slack.send_message(v: std.collections.at(params.items, 0))
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
    let rexrap = decompile(&compile(src)).expect("decompile");
    assert!(
        rexrap.contains("at("),
        "explicit at not preserved: {rexrap}"
    );
    assert_round_trips(src);
}
#[test]
fn effectful_postfix_access_outside_compute_is_rejected() {
    // the effectful call inside an access chain is still rejected in a declarative position.
    let src = r#"
        workflow "Bad" v1 {
            slack.send_message(text: std.exec.http_get(params.url))
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
            slack.send_message(at: std.exec.now())
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
                slack.send_message(text: "ok")
            }
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
}
