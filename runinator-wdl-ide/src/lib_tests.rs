//! completion and hover behaviour, driven against source buffers with a cursor marker.
//!
//! these moved out of `runinator-wdl`'s suite with the modules they cover: they assert what an
//! editor offers at a position, which is independent of what the compiler makes of the program.

use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, ResultMetadata,
    RuninatorType,
};

use crate::{
    WdlCompletionRequest, WdlCompletionResponse, WdlHoverRequest, WdlHoverResponse,
    complete_source, hover_source,
};

fn completion_labels(src: &str, marker: &str) -> Vec<String> {
    completion_labels_with_providers(src, marker, completion_providers())
}

fn hover_at(src: &str, marker: &str) -> WdlHoverResponse {
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    hover_source(WdlHoverRequest {
        source,
        cursor_byte: cursor,
        providers: completion_providers(),
        settings: Vec::new(),
    })
    .expect("hover")
}

fn completion_labels_with_providers(
    src: &str,
    marker: &str,
    providers: Vec<ProviderMetadata>,
) -> Vec<String> {
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    complete_source(WdlCompletionRequest {
        source,
        cursor_byte: cursor,
        providers,
        settings: Vec::new(),
    })
    .items
    .into_iter()
    .map(|item| item.label)
    .collect()
}

fn completion_providers() -> Vec<ProviderMetadata> {
    let issue_type = RuninatorType::open_structure(
        [
            ("key", RuninatorType::String),
            (
                "fields",
                RuninatorType::open_structure(
                    [("summary", RuninatorType::String)],
                    RuninatorType::Any,
                ),
            ),
        ],
        RuninatorType::Any,
    );
    vec![
        ProviderMetadata {
            name: "jira".into(),
            actions: vec![
                ActionMetadata::new("search", "Search Jira issues")
                    .with_parameters(vec![
                        ParameterMetadata::required("base_url", RuninatorType::String),
                        ParameterMetadata::required("token", RuninatorType::String).secret(),
                        ParameterMetadata::optional("email", RuninatorType::String),
                        ParameterMetadata::required("jql", RuninatorType::String),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("issues", RuninatorType::array(issue_type)),
                        ResultMetadata::new("total", RuninatorType::Integer),
                    ]),
                ActionMetadata::new("transition", "Transition a Jira issue").with_parameters(vec![
                    ParameterMetadata::required("key", RuninatorType::String),
                ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        },
        ProviderMetadata {
            name: "slack".into(),
            actions: vec![ActionMetadata::new("send_message", "Send a Slack message")],
            metadata: ProviderRuntimeMetadata::default(),
        },
    ]
}

#[test]
fn completes_provider_names_at_action_position() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            ji<>
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"jira".to_string()));
    assert!(labels.contains(&"slack".to_string()));
}

#[test]
fn completes_language_constructs_at_bare_position() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            <>
        }
    "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            <>
        }
    "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(labels.contains(&"node"), "labels: {labels:?}");
    assert!(labels.contains(&"compute"), "labels: {labels:?}");
    assert!(labels.contains(&"if"), "labels: {labels:?}");
    assert!(labels.contains(&"for"), "labels: {labels:?}");
    assert!(labels.contains(&"trigger cron"), "labels: {labels:?}");
    assert!(labels.contains(&"gate condition"), "labels: {labels:?}");
    for generated in ["approve", "fail", "input", "race", "set", "until"] {
        assert!(
            labels.contains(&generated),
            "missing {generated}: {labels:?}"
        );
    }
    assert!(response.items.iter().any(|item| item.label == "node"
        && item.kind == "keyword"
        && item.is_snippet
        && item.insert_text.contains("provider")));
}

#[test]
fn gate_completion_and_hover_follow_timeout_policy_syntax() {
    let response = complete_source(WdlCompletionRequest {
        source: "workflow \"Gate\" { }".into(),
        cursor_byte: 18,
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let gate = response
        .items
        .iter()
        .find(|item| item.label == "gate condition")
        .expect("gate completion");
    assert!(gate.insert_text.contains("on_timeout ${policy}"));

    let hover = hover_at(
        "workflow \"Gate\" { gate manual timeout 30s <>on_timeout continue }",
        "<>",
    );
    assert_eq!(hover.title, "on_timeout");
    assert_eq!(hover.kind, "keyword");
}

#[test]
fn hovers_provider_action_docs_and_signature() {
    let hover = hover_at(
        r#"
        workflow "Hover" v1 {
            node search <- jira.<>search(base_url: "", token: "", jql: "project = RUNI")
        }
    "#,
        "<>",
    );
    assert_eq!(hover.title, "jira.search");
    assert_eq!(hover.kind, "action");
    assert_eq!(hover.documentation.as_deref(), Some("Search Jira issues"));
    assert!(
        hover
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("jql: string") && detail.contains("struct")),
        "detail: {:?}",
        hover.detail
    );
}

#[test]
fn hovers_inferred_node_result_field_type() {
    let hover = hover_at(
        r#"
        workflow "Hover" v1 {
            node search <- jira.search(base_url: "", token: "", jql: "project = RUNI")
            node inspect <- compute {
                return search.<>issues
            }
        }
    "#,
        "<>",
    );
    assert_eq!(hover.title, "issues");
    assert_eq!(hover.kind, "field");
    assert!(
        hover
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("optional") && detail.contains("[]")),
        "detail: {:?}",
        hover.detail
    );
}

#[test]
fn completes_std_modules_and_functions() {
    let modules = completion_labels(
        r#"
        workflow "Complete" v1 {
            compute { return std.<> }
        }
    "#,
        "<>",
    );
    assert!(
        modules.contains(&"strings".to_string()),
        "modules: {modules:?}"
    );
    assert!(
        modules.contains(&"collections".to_string()),
        "modules: {modules:?}"
    );

    let functions = completion_labels(
        r#"
        workflow "Complete" v1 {
            compute { return std.strings.<> }
        }
    "#,
        "<>",
    );
    assert!(
        functions.contains(&"upper".to_string()),
        "functions: {functions:?}"
    );
    assert!(
        !functions.contains(&"add".to_string()),
        "math leaked into strings: {functions:?}"
    );
}

#[test]
fn completes_std_modules_when_std_provider_metadata_exists() {
    let mut providers = completion_providers();
    providers.push(ProviderMetadata {
        name: "std".into(),
        actions: vec![
            ActionMetadata::new("run", "evaluate a pure compute program"),
            ActionMetadata::new("exec", "execute an effectful compute program"),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    });
    let modules = completion_labels_with_providers(
        r#"
        workflow "Complete" v1 {
            compute { return std.<> }
        }
    "#,
        "<>",
        providers,
    );
    assert!(
        modules.contains(&"collections".to_string()),
        "std provider metadata hid std modules: {modules:?}"
    );
    assert!(
        !modules.contains(&"run".to_string()),
        "std provider actions leaked into std module completion: {modules:?}"
    );
}

#[test]
fn completes_provider_actions_after_dot() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            jira.<>
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"search".to_string()));
    assert!(labels.contains(&"transition".to_string()));
}

#[test]
fn completes_aliased_std_module_leaves() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            import std.strings as s
            compute { return s.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"upper".to_string()), "labels: {labels:?}");
    assert!(
        !labels.contains(&"add".to_string()),
        "math leaked through strings alias: {labels:?}"
    );
}

#[test]
fn completes_bare_intrinsics_from_unaliased_import() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            import std.strings
            compute { return up<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"upper".to_string()), "labels: {labels:?}");
    // a module that was not imported stays out of bare scope.
    assert!(
        !labels.contains(&"merge".to_string()),
        "un-imported module leaked into bare scope: {labels:?}"
    );
}

#[test]
fn does_not_complete_unimported_intrinsics_bare() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            compute { return up<> }
        }
    "#,
        "<>",
    );
    assert!(
        !labels.contains(&"upper".to_string()),
        "unqualified intrinsic offered without import: {labels:?}"
    );
}

#[test]
fn completes_user_functions_bare() {
    let labels = completion_labels(
        r#"
        fn shout(text: string) -> string = text
        workflow "Complete" v1 {
            compute { return sh<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"shout".to_string()), "labels: {labels:?}");
}

#[test]
fn completes_node_labels_bare() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "tickets" { total: <> }
        }
    "#,
        "<>",
    );
    assert!(
        labels.contains(&"tickets".to_string()),
        "labels: {labels:?}"
    );
}

#[test]
fn completes_node_labels_as_transition_targets() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node first <- console.run(command: "one") -> <>
            node cleanup <- console.run(command: "cleanup")
        }
    "#,
        "<>",
    );
    assert!(
        labels.contains(&"cleanup".to_string()),
        "labels: {labels:?}"
    );
}

#[test]
fn completes_edge_outcomes_in_edges_blocks() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            node first <- console.run(command: "one")
            edges {
                <>
            }
            node cleanup <- console.run(command: "cleanup")
        }
    "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            node first <- console.run(command: "one")
            edges {
                <>
            }
            node cleanup <- console.run(command: "cleanup")
        }
    "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(labels.contains(&"ok"), "labels: {labels:?}");
    assert!(labels.contains(&"when"), "labels: {labels:?}");
    assert!(response.items.iter().any(|item| item.label == "ok"
        && item.kind == "edge"
        && item.is_snippet
        && item.insert_text == "ok -> ${target}"));
}

#[test]
fn completes_terminal_and_node_targets_after_arrow() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            node first <- console.run(command: "one") -> <>
            node cleanup <- console.run(command: "cleanup")
        }
    "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            node first <- console.run(command: "one") -> <>
            node cleanup <- console.run(command: "cleanup")
        }
    "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(labels.contains(&"done"), "labels: {labels:?}");
    assert!(labels.contains(&"fail"), "labels: {labels:?}");
    assert!(labels.contains(&"cleanup"), "labels: {labels:?}");
    assert!(
        response
            .items
            .iter()
            .any(|item| item.label == "cleanup" && item.kind == "node")
    );
    assert!(
        response
            .items
            .iter()
            .any(|item| item.label == "done" && item.kind == "target")
    );
}

#[test]
fn completes_missing_action_arguments() {
    let response = complete_source(WdlCompletionRequest {
        source: r#"
        workflow "Complete" v1 {
            jira.search(base_url: params.base, <>)
        }
        "#
        .replace("<>", ""),
        cursor_byte: r#"
        workflow "Complete" v1 {
            jira.search(base_url: params.base, <>)
        }
        "#
        .find("<>")
        .expect("marker"),
        providers: completion_providers(),
        settings: Vec::new(),
    });
    let labels = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();
    assert!(!labels.contains(&"base_url"));
    assert!(labels.contains(&"token"));
    // token is a required string, so the snippet pre-fills quotes with an editable field inside.
    assert!(response.items.iter().any(|item| item.label == "token"
        && item.is_snippet
        && item.insert_text == "token: \"${}\""));
}

#[test]
fn completes_nested_input_fields() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            params {
                jira: { base_url: string, token: string }
            }
            jira.search(base_url: params.jira.<>, token: params.jira.token, jql: "x")
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"base_url".to_string()));
    assert!(labels.contains(&"token".to_string()));
}

#[test]
fn completes_provider_result_outputs() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "tickets" { issues: tickets.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"issues".to_string()));
    assert!(labels.contains(&"total".to_string()));
}

#[test]
fn explicit_binding_type_overrides_provider_results() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets: { custom: string } <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "tickets" { value: tickets.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"custom".to_string()));
    assert!(!labels.contains(&"issues".to_string()));
}

#[test]
fn completes_loop_variable_fields_from_array_source() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            for item in tickets.issues limit 10 {
                emit "ticket" { key: item.<> }
            }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"key".to_string()));
    assert!(labels.contains(&"fields".to_string()));
}

#[test]
fn completes_prev_output_fields_after_action() {
    // `prev` resolves to the source-order predecessor node's output type.
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            emit "prev" { total: prev.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"issues".to_string()));
    assert!(labels.contains(&"total".to_string()));
}

#[test]
fn prev_has_no_fields_at_first_node() {
    // the first node has no predecessor, so `prev` stays opaque (Any) with no known fields.
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            emit "prev" { total: prev.<> }
        }
    "#,
        "<>",
    );
    assert!(!labels.contains(&"issues".to_string()));
    assert!(!labels.contains(&"total".to_string()));
}

#[test]
fn prev_has_no_fields_after_control_flow() {
    // after a loop, the predecessor is ambiguous, so `prev` falls back to Any.
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            for item in tickets.issues limit 10 {
                emit "ticket" { key: item.key }
            }
            emit "prev" { total: prev.<> }
        }
    "#,
        "<>",
    );
    assert!(!labels.contains(&"issues".to_string()));
    assert!(!labels.contains(&"total".to_string()));
}

#[test]
fn completes_loop_variable_fields_from_prev_array() {
    // `prev` typing composes with loop-variable element typing: iterating `prev.issues` binds the
    // loop variable to the array element struct.
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            for item in prev.issues limit 10 {
                emit "ticket" { key: item.<> }
            }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"key".to_string()));
    assert!(labels.contains(&"fields".to_string()));
}

#[test]
fn completes_provider_actions_in_incomplete_source() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            jira.<>
    "#,
        "<>",
    );
    assert!(labels.contains(&"search".to_string()));
}

#[test]
fn suppresses_completion_inside_plain_string() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            emit "jira.<>"
        }
    "#,
        "<>",
    );
    assert!(labels.is_empty());
}

#[test]
fn completes_map_result_element_fields() {
    // completion infers a higher-order result element type (through namespace resolution), so the
    // projected struct's fields are offered on the loop variable.
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            node tickets <- jira.search(base_url: "https://jira", token: "t", jql: "x")
            for row in std.collections.map(tickets.issues, i => { title: i.key }) limit 10 {
                emit "row" { t: row.<> }
            }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"title".to_string()), "labels: {labels:?}");
}

#[test]
fn completes_run_context_fields() {
    let labels = completion_labels(
        r#"
        workflow "Complete" v1 {
            emit "run" { id: run.<> }
        }
    "#,
        "<>",
    );
    assert!(labels.contains(&"run_id".to_string()));
    assert!(labels.contains(&"workflow_id".to_string()));
}

fn setting_completion(src: &str, marker: &str) -> WdlCompletionResponse {
    use runinator_models::settings::{SettingKind, SettingSummary};
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    let settings = vec![
        SettingSummary {
            scope: "github".into(),
            name: "token".into(),
            kind: SettingKind::Secret,
            expires_at: None,
        },
        SettingSummary {
            scope: "github".into(),
            name: "base_url".into(),
            kind: SettingKind::Config,
            expires_at: None,
        },
        SettingSummary {
            scope: "slack".into(),
            name: "webhook".into(),
            kind: SettingKind::Secret,
            expires_at: None,
        },
    ];
    complete_source(WdlCompletionRequest {
        source,
        cursor_byte: cursor,
        providers: completion_providers(),
        settings,
    })
}

#[test]
fn completes_secret_scopes() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            emit "out" { token: secret.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert!(labels.contains(&"github".to_string()));
    assert!(labels.contains(&"slack".to_string()));
}

#[test]
fn completes_secret_names_within_scope() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            emit "out" { token: secret.github.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    // only the secret-kind name in the github scope is suggested, not the config slot.
    assert_eq!(labels, vec!["token".to_string()]);
}

#[test]
fn completes_config_scopes_separately_from_secrets() {
    let labels = setting_completion(
        r#"
        workflow "Complete" v1 {
            emit "out" { url: config.github.<> }
        }
    "#,
        "<>",
    )
    .items
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert_eq!(labels, vec!["base_url".to_string()]);
}

#[test]
fn parameter_defaults_use_typed_placeholders() {
    use runinator_models::providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
    };
    let providers = vec![ProviderMetadata {
        name: "demo".into(),
        actions: vec![ActionMetadata::new("run", "demo").with_parameters(vec![
            ParameterMetadata::required("count", RuninatorType::Integer),
            ParameterMetadata::required("flag", RuninatorType::Boolean),
            ParameterMetadata::optional("name", RuninatorType::String).with_default("ada"),
        ])],
        metadata: ProviderRuntimeMetadata::default(),
    }];
    let src = "workflow \"D\" v1 {\n    demo.run()\n}";
    let cursor = src.find("()").expect("marker") + 1;
    let inserts = complete_source(WdlCompletionRequest {
        source: src.to_string(),
        cursor_byte: cursor,
        providers,
        settings: Vec::new(),
    })
    .items
    .into_iter()
    .map(|item| (item.label, item.insert_text))
    .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        inserts.get("count").map(String::as_str),
        Some("count: ${0}")
    );
    assert_eq!(
        inserts.get("flag").map(String::as_str),
        Some("flag: ${false}")
    );
    // a concrete default becomes a pre-selected literal.
    assert_eq!(
        inserts.get("name").map(String::as_str),
        Some("name: ${\"ada\"}")
    );
}

// a packaged-function provider is an ordinary dotted-name provider as far as the editor is
// concerned, which is the point: the catalog carries `functions.<pkg>` and nothing here needs to
// know it came from a published package rather than a running worker.
fn function_providers() -> Vec<ProviderMetadata> {
    vec![ProviderMetadata {
        name: "functions.image_tools".into(),
        actions: vec![
            ActionMetadata::new("resize", "Resize an image")
                .with_parameters(vec![
                    ParameterMetadata::required("source", RuninatorType::String),
                    ParameterMetadata::optional("width", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new("uri", RuninatorType::String)]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    }]
}

fn hover_with(
    src: &str,
    marker: &str,
    providers: Vec<ProviderMetadata>,
) -> Option<WdlHoverResponse> {
    let cursor = src.find(marker).expect("marker");
    let source = src.replacen(marker, "", 1);
    hover_source(WdlHoverRequest {
        source,
        cursor_byte: cursor,
        providers,
        settings: Vec::new(),
    })
}

#[test]
fn hovers_a_three_segment_packaged_function_call() {
    let hover = hover_with(
        r#"
        workflow "Hover" v1 {
            functions.image_tools.re<>size(source: "a.png")
        }
    "#,
        "<>",
        function_providers(),
    )
    .expect("hover");

    // the split is on the *last* dot, matching the grammar: everything before it is the provider.
    // splitting on the first would look for an action named `image_tools.resize` on a provider
    // named `functions`, and find neither.
    assert_eq!(hover.title, "functions.image_tools.resize");
    assert_eq!(hover.kind, "action");
    assert_eq!(hover.documentation.as_deref(), Some("Resize an image"));
}

#[test]
fn hovering_the_package_half_reports_the_provider() {
    let hover = hover_with(
        r#"
        workflow "Hover" v1 {
            functions.image<>_tools.resize(source: "a.png")
        }
    "#,
        "<>",
        function_providers(),
    )
    .expect("hover");

    assert_eq!(hover.title, "functions.image_tools");
    assert_eq!(hover.kind, "provider");
}

#[test]
fn completes_packaged_function_exports() {
    let labels = completion_labels_with_providers(
        r#"
        workflow "Complete" v1 {
            functions.image_tools.<>
        }
    "#,
        "<>",
        function_providers(),
    );
    assert!(
        labels.iter().any(|label| label == "resize"),
        "expected the export among {labels:?}"
    );
}

#[test]
fn completes_arguments_inside_a_three_segment_call() {
    let labels = completion_labels_with_providers(
        r#"
        workflow "Complete" v1 {
            functions.image_tools.resize(<>)
        }
    "#,
        "<>",
        function_providers(),
    );
    // the call context walks the provider back over every dotted segment too; reading only one
    // would look up a provider named `image_tools` and offer nothing.
    assert!(
        labels.iter().any(|label| label == "source"),
        "expected the declared arguments among {labels:?}"
    );
    assert!(labels.iter().any(|label| label == "width"), "{labels:?}");
}

#[test]
fn a_dotted_value_path_is_still_not_mistaken_for_a_provider() {
    // removing the "no dotted providers" guard could have made `config.x.<cursor>` complete as an
    // action member. what keeps it honest is that the prefix has to resolve to a real provider.
    let labels = completion_labels_with_providers(
        r#"
        workflow "Complete" v1 {
            node a <- functions.image_tools.resize(source: config.database.<>)
        }
    "#,
        "<>",
        function_providers(),
    );
    assert!(
        !labels.iter().any(|label| label == "resize"),
        "a config path must not offer function exports: {labels:?}"
    );
}
