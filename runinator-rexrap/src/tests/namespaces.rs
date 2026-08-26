//! namespaces and imports: qualified subflow targets, `import std`, module aliases, and the
//! prefixes that are rejected.

use super::*;

#[test]
fn stable_workflow_key_round_trips_independently_of_display_name() {
    let src = r#"
        namespace acme.billing {
            workflow "Nightly reconciliation" v1 {
                key billing_reconcile

                do { return }
            }
        }
    "#;
    let definition = compile(src);
    assert_eq!(definition.name, "Nightly reconciliation");
    assert_eq!(definition.key.as_deref(), Some("billing_reconcile"));
    assert_eq!(
        definition.artifact_path().qualified(),
        "acme.billing.billing_reconcile"
    );
    assert_round_trips(src);
}

#[test]
fn workflow_namespace_and_qualified_subflow_round_trip() {
    // a `namespace` header rides in metadata, and a qualified subflow target keeps its dotted name.
    let src = r#"
        namespace core_sdlc {
            workflow "Caller" v1 {

                do {
                    subflow("core_sdlc.ticket_work", params: { id: params.id })
                }
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
fn typed_workflow_import_resolves_an_alias_and_keeps_its_revision_selector() {
    let definition = compile(
        r#"
        workflow "Caller" v1 {
            import workflow acme.billing.reconcile @revision(42) as reconcile

            do {
                subflow(reconcile, params: { id: params.id })
            }
        }
        "#,
    );
    let graph = graph_value(&definition);
    let subflow = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["kind"] == "subflow")
        .expect("subflow node");
    assert_eq!(
        subflow["subflow"]["workflow_name"],
        "acme.billing.reconcile"
    );
    assert_eq!(subflow["subflow"]["revision"], 42);
}

#[test]
fn typed_settings_import_resolves_config_and_secret_aliases() {
    let definition = compile(
        r#"
        workflow "Settings alias" v1 {
            import settings acme.shared as shared

            do {
                slack.send_message(
                    text: shared.message,
                    token: shared.secret.slack_token,
                )
            }
        }
        "#,
    );
    let action = graph_value(&definition)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["kind"] == "action")
        .unwrap()["action"]
        .clone();
    assert_eq!(
        action["configuration"]["text"]["$ref"]["config"],
        serde_json::json!(["acme.shared", "message"])
    );
    assert_eq!(
        action["configuration"]["token"],
        "secret://acme.shared/slack_token"
    );
}

#[test]
fn compilation_rejects_legacy_artifact_authoring() {
    let options = default_test_options();
    let missing_identity = r#"
        workflow "Legacy" v1 {
            do { return }
        }
    "#;
    let error =
        crate::compile_str(missing_identity, &options).expect_err("stable identity is required");
    assert!(error.to_string().contains("stable `key`"));

    let bare_setting = r#"
        namespace acme.billing {
            workflow "Legacy setting" v1 {
                key legacy_setting
                do { console.run(command: config.app.command) }
            }
        }
    "#;
    let error =
        crate::compile_str(bare_setting, &options).expect_err("typed settings import is required");
    assert!(error.to_string().contains("typed `import settings"));
}
#[test]
fn import_std_brings_intrinsics_into_bare_scope() {
    // `import std` opens the whole standard library so prefix calls need no qualification; the
    // decompiler still canonicalizes to the qualified form, so the round trip is stable.
    let src = r#"
        workflow "Imp" v1 {
            import std

            do {
                compute {
                    let total = add(params.a, params.b)
                    return upper(params.name)
                }
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let module = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "invocation")
        .unwrap()["parameters"]["module"]
        .to_string();
    // The module holds bare runtime leaves, never the std prefix.
    assert!(module.contains("\"add\""), "module: {module}");
    assert!(
        !module.contains("std.math"),
        "module leaked namespace: {module}"
    );
    assert_round_trips(src);
}
#[test]
fn aliased_module_import_resolves() {
    // `import std.strings as s` binds `s.upper(x)` to the strings module.
    let src = r#"
        workflow "Alias" v1 {
            import std.strings as s

            do {
                slack.send_message(text: s.upper(params.name))
            }
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

            do {
                github.repos.create_pr(title: params.title)
            }
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

            do {
                compute { return add(1, 2) }
            }
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

            do {
                compute { return std.math.upper(params.name) }
            }
        }
    "#,
    );
    assert!(
        message.contains("std.strings"),
        "expected a hint to the real module, got: {message}"
    );
}
