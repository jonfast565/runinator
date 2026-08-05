//! node-level lowering: waits, annotations, hyphenated providers, and config/secret references.

use super::*;

#[test]
fn round_trips_expression_wait() {
    // wait can take a literal duration or an expression that yields seconds.
    let src = r#"
        workflow "DynWait" v1 {
            params { poll: { interval: int } }
            node seed <- console.run(command: "seed")
            wait params.poll.interval until "ready"
            node done <- console.run(command: "done")
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
            node run <- ai-command.claude_code(prompt: "hi").timeout(60s)
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
            node go <- console.run(command: "x", url: config.api.url, token: secret.github.token)
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
            node go <- console.run(command: inline("python", ```
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
