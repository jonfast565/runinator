//! namespaces and imports: qualified subflow targets, `import std`, module aliases, and the
//! prefixes that are rejected.

use super::*;

#[test]
fn workflow_namespace_and_qualified_subflow_round_trip() {
    // a `namespace` header rides in metadata, and a qualified subflow target keeps its dotted name.
    let src = r#"
        namespace core_sdlc {
            workflow "Caller" v1 {
                subflow("core_sdlc.ticket_work", params: { id: params.id })
            }
        }
    "#;
    let definition = compile(src);
    assert_eq!(definition.namespace.as_deref(), Some("core_sdlc"));
    let graph = graph_value(&definition);
    let subflow = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "subflow")
        .expect("subflow node");
    assert_eq!(subflow["subflow"]["workflow_name"], "core_sdlc.ticket_work");
    assert_round_trips(src);
}
#[test]
fn import_std_brings_intrinsics_into_bare_scope() {
    // `import std` opens the whole standard library so prefix calls need no qualification; the
    // decompiler still canonicalizes to the qualified form, so the round trip is stable.
    let src = r#"
        workflow "Imp" v1 {
            import std
            compute {
                let total = add(params.a, params.b)
                return upper(params.name)
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let program = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]["program"]
        .to_string();
    // the compiled program holds bare runtime leaves, never the std prefix.
    assert!(program.contains("\"add\""), "program: {program}");
    assert!(
        !program.contains("std.math"),
        "program leaked namespace: {program}"
    );
    assert_round_trips(src);
}
#[test]
fn aliased_module_import_resolves() {
    // `import std.strings as s` binds `s.upper(x)` to the strings module.
    let src = r#"
        workflow "Alias" v1 {
            import std.strings as s
            slack.send_message(text: s.upper(params.name))
        }
    "#;
    let definition = compile(src);
    let config = graph_value(&definition)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]["configuration"]
        .to_string();
    assert!(config.contains("\"$call\":\"upper\""), "config: {config}");
    assert_round_trips(src);
}
#[test]
fn namespaced_provider_action_round_trips() {
    // a dotted provider path keeps every leading segment as the provider; the trailing segment is
    // the function.
    let src = r#"
        workflow "NsAction" v1 {
            github.repos.create_pr(title: params.title)
        }
    "#;
    let definition = compile(src);
    let action = graph_value(&definition)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap()["action"]
        .clone();
    assert_eq!(action["provider"], "github.repos");
    assert_eq!(action["function"], "create_pr");
    assert_round_trips(src);
}
#[test]
fn bare_intrinsic_prefix_call_is_rejected() {
    let (_, message) = expect_semantic(
        r#"
        workflow "Bare" v1 {
            compute { return add(1, 2) }
        }
    "#,
    );
    assert!(message.contains("must be qualified"), "got: {message}");
}
#[test]
fn wrong_std_module_is_rejected() {
    let (_, message) = expect_semantic(
        r#"
        workflow "WrongMod" v1 {
            compute { return std.math.upper(params.name) }
        }
    "#,
    );
    assert!(
        message.contains("std.strings"),
        "expected a hint to the real module, got: {message}"
    );
}
