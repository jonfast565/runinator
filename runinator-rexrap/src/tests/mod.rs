//! the rexrap language tests, partitioned by subject.
//!
//! most of these are round-trip assertions: source lowers to the json workflow model and decompiles
//! back to source that formats identically. that is the crate's central contract, so the helpers
//! that express it (`assert_round_trips` and its variants) live here and every submodule picks them
//! up through its `use super` glob.
//!
//! editor-surface behaviour (completion, hover) is not here — it moved out with `runinator-rexrap-ide`.

mod aliases;
mod calls;
mod comments;
mod compute_block;
mod control_flow;
mod diagnostics;
mod explicit_form;
mod expressions;
mod fixtures;
mod format;
mod fragments;
mod functions;
mod graph;
mod includes;
mod inputs;
mod modifiers;
mod namespaces;
mod nodes;
mod packaged_functions;
mod parsing;
mod schedules;
mod settings;
mod spans;
mod task_bindings;
mod triggers;
mod types;
mod validation;

use crate::{
    CompileOptions, DecompileOptions, RexRapError, RexRapFragmentKind, WorkflowSignature,
    analyze_source as analyze_source_strict, compile_all_str as compile_all_strict,
    compile_str as compile_str_strict,
    compile_str_with_diagnostics as compile_str_with_diagnostics_strict, decompile, decompile_with,
    decompile_with_spans, evaluate_fragment, evaluate_fragment_with_functions, format_str,
    parse_document, validate_fragment, validate_fragment_with_functions,
    workflow_signature_from_source,
};
use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, ResultMetadata,
    RuninatorType,
};
use runinator_models::value::Value;
use std::{fs, time::SystemTime};

/// Most language tests predate mandatory artifact identity and focus on a different surface. Give
/// those fixtures a stable identity before they enter the real compiler; namespace-specific tests
/// call the crate API directly when absence itself is the behavior under test.
fn strict_test_source(src: &str) -> String {
    let Ok(document) = parse_document(src) else {
        return src.to_string();
    };
    let needs_namespace = document
        .workflows
        .iter()
        .any(|workflow| workflow.namespace.is_none());
    let mut insertions = document
        .workflows
        .iter()
        .filter(|workflow| workflow.key.is_none())
        .filter_map(|workflow| {
            let tail = src.get(workflow.span.start..workflow.span.end)?;
            let relative = tail.match_indices("do").find_map(|(offset, _)| {
                let before = tail[..offset].chars().next_back();
                let after = tail[offset + 2..].chars().next();
                let boundary = |ch: Option<char>| {
                    ch.is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
                };
                let opens_block = tail[offset + 2..].trim_start().starts_with('{');
                (boundary(before) && boundary(after) && opens_block).then_some(offset)
            })?;
            Some((
                workflow.span.start + relative,
                "key runinator_test\n".to_string(),
            ))
        })
        .collect::<Vec<_>>();
    insertions.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
    let mut strict = src.to_string();
    for (offset, identity) in insertions {
        strict.insert_str(offset, &identity);
    }
    if needs_namespace {
        let offset = strict
            .strip_prefix("language rexrap-1")
            .and_then(|tail| {
                tail.find('\n')
                    .map(|line| "language rexrap-1".len() + line + 1)
            })
            .unwrap_or(0);
        strict.insert_str(offset, "namespace runinator.tests\n");
    }
    strict_test_resources(strict)
}

fn strict_test_resources(mut source: String) -> String {
    let mut imports = std::collections::BTreeSet::new();

    for root in ["config", "secret"] {
        let mut scopes = std::collections::BTreeSet::new();
        let needle = format!("{root}.");
        let mut offset = 0;
        while let Some(found) = source[offset..].find(&needle) {
            let start = offset + found + needle.len();
            let scope = source[start..]
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect::<String>();
            let has_field = source[start + scope.len()..].starts_with('.');
            if !scope.is_empty() && has_field {
                scopes.insert(scope);
            }
            offset = start.max(offset + 1);
        }
        for scope in scopes {
            imports.insert(format!("import settings {scope} as {scope}"));
            let authored = format!("{root}.{scope}.");
            let alias = if root == "secret" {
                format!("{scope}.secret.")
            } else {
                format!("{scope}.")
            };
            source = source.replace(&authored, &alias);
        }
    }

    while let Some(start) = source.find("subflow(\"") {
        let name_start = start + "subflow(\"".len();
        let Some(end) = source[name_start..].find('\"').map(|end| name_start + end) else {
            break;
        };
        let path = source[name_start..end].to_string();
        let durable_path = if path == "Ticket Work" {
            "core_sdlc.ticket_work".to_string()
        } else {
            path.clone()
        };
        let alias = durable_path
            .rsplit('.')
            .next()
            .unwrap_or("workflow")
            .replace('-', "_");
        imports.insert(format!("import workflow {durable_path} as {alias}"));
        source.replace_range(start..=end, &format!("subflow({alias}"));
    }

    let mut search_from = 0;
    while let Some(found) = source[search_from..].find("functions.") {
        let start = search_from + found;
        let Some(open) = source[start..].find('(').map(|open| start + open) else {
            break;
        };
        let call = source[start..open].to_string();
        if !call
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        {
            search_from = start + "functions.".len();
            continue;
        }
        let Some((package, export)) = call
            .strip_prefix("functions.")
            .and_then(|call| call.rsplit_once('.'))
        else {
            search_from = open + 1;
            continue;
        };
        let alias = package
            .rsplit('.')
            .next()
            .unwrap_or("package")
            .replace('-', "_");
        imports.insert(format!("import functions {package} as {alias}"));
        source.replace_range(start..open, &format!("{alias}.{export}"));
        search_from = start + alias.len() + export.len() + 2;
    }

    if imports.is_empty() {
        return source;
    }
    let import_text = format!("{}\n", imports.into_iter().collect::<Vec<_>>().join("\n"));
    let Ok(document) = parse_document(&source) else {
        return source;
    };
    let mut offsets = document
        .workflows
        .iter()
        .filter_map(|workflow| {
            let tail = source.get(workflow.span.start..workflow.span.end)?;
            tail.match_indices("do").find_map(|(offset, _)| {
                tail[offset + 2..]
                    .trim_start()
                    .starts_with('{')
                    .then_some(workflow.span.start + offset)
            })
        })
        .collect::<Vec<_>>();
    offsets.sort_by_key(|offset| std::cmp::Reverse(*offset));
    for offset in offsets {
        source.insert_str(offset, &import_text);
    }
    source
}

fn compile_str(
    src: &str,
    options: &CompileOptions,
) -> Result<runinator_models::workflows::WorkflowDefinition, RexRapError> {
    compile_str_strict(&strict_test_source(src), options)
}

fn compile_all_str(
    src: &str,
    options: &CompileOptions,
) -> Result<Vec<runinator_models::workflows::WorkflowDefinition>, RexRapError> {
    compile_all_strict(&strict_test_source(src), options)
}

fn compile_str_with_diagnostics(
    src: &str,
    options: &CompileOptions,
) -> Result<
    (
        runinator_models::workflows::WorkflowDefinition,
        Vec<crate::Diagnostic>,
    ),
    RexRapError,
> {
    compile_str_with_diagnostics_strict(&strict_test_source(src), options)
}

fn analyze_source(src: &str) -> Result<Vec<crate::Diagnostic>, RexRapError> {
    analyze_source_strict(&strict_test_source(src))
}

/// compile and return the `Semantic` error's span and message, failing otherwise.
fn expect_semantic(src: &str) -> (crate::Span, String) {
    match compile_str(src, &CompileOptions::default()) {
        Err(RexRapError::Semantic { span, message }) => (span, message),
        other => panic!("expected semantic error, got {other:?}"),
    }
}
fn compile(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    compile_str(src, &default_test_options()).expect("compile")
}
fn compile_with_providers(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    let options = CompileOptions {
        providers: runinator_provider_catalog::metadata(),
        workflow_signatures: test_workflow_signatures(),
        ..CompileOptions::default()
    };
    compile_str(src, &options).expect("compile with providers")
}
fn default_test_options() -> CompileOptions {
    CompileOptions {
        workflow_signatures: test_workflow_signatures(),
        ..CompileOptions::default()
    }
}
fn test_workflow_signatures() -> Vec<WorkflowSignature> {
    ["Ticket Work", "Child", "core_sdlc.ticket_work"]
        .into_iter()
        .map(|name| WorkflowSignature {
            name: name.to_string(),
            input: RuninatorType::Any,
            output: RuninatorType::Any,
        })
        .collect()
}
fn action_config_value<'a>(
    definition: &'a runinator_models::workflows::WorkflowDefinition,
    key: &str,
) -> &'a Value {
    definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Action)
        .and_then(|node| node.action.as_ref())
        .and_then(|action| action.configuration.get(key))
        .unwrap_or_else(|| panic!("missing action configuration key '{key}'"))
}
fn graph_value(definition: &runinator_models::workflows::WorkflowDefinition) -> serde_json::Value {
    serde_json::to_value(&definition.definition).expect("serialize graph")
}
fn replace_node_id(value: &mut serde_json::Value, old: &str, new: &str) {
    match value {
        serde_json::Value::String(text) if text == old => {
            *text = new.to_string();
        }
        serde_json::Value::Array(items) => {
            for item in items {
                replace_node_id(item, old, new);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                replace_node_id(item, old, new);
            }
        }
        _ => {}
    }
}
/// compile and require a semantic error, returning its message.
fn expect_semantic_error(src: &str) -> String {
    match compile_str(src, &CompileOptions::default()) {
        Err(RexRapError::Semantic { message, .. }) => message,
        other => panic!("expected semantic error, got {other:?}"),
    }
}
/// whether `first` and `second` both appear in `text` with `first` preceding `second`. used by
/// layout-tolerant assertions now that arguments lay out one per line.
fn ordered(text: &str, first: &str, second: &str) -> bool {
    match (text.find(first), text.find(second)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}
/// compile -> decompile -> compile and assert the normalized graphs match.
fn assert_round_trips(src: &str) {
    let first = compile(src);
    let rexrap = decompile(&first).expect("decompile");
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{rexrap}"));
    let normalized_first = runinator_workflows::normalize_definition(first.definition.clone());
    let normalized_second = runinator_workflows::normalize_definition(second.definition.clone());
    assert_eq!(
        normalized_first, normalized_second,
        "round trip diverged\n--- decompiled ---\n{rexrap}"
    );
}
/// like `assert_round_trips`, but compares the node *set* rather than array order. node order
/// carries no execution meaning (the graph is followed via `start` + transitions), and a
/// decompiler that re-nests branches legitimately renders nodes in a different order.
fn assert_round_trips_unordered(src: &str) {
    let first = compile(src);
    let rexrap = decompile(&first).expect("decompile");
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{rexrap}"));

    let sorted_nodes = |definition: &runinator_models::workflows::WorkflowGraph| {
        let normalized = runinator_workflows::normalize_definition(definition.clone());
        let value = serde_json::to_value(&normalized).expect("serialize graph");
        let mut nodes = value
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        nodes.sort_by(|a, b| {
            let id = |v: &serde_json::Value| {
                v.get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            id(a).cmp(&id(b))
        });
        (value.get("start").cloned(), nodes)
    };

    assert_eq!(
        sorted_nodes(&first.definition),
        sorted_nodes(&second.definition),
        "round trip diverged (order-insensitive)\n--- decompiled ---\n{rexrap}"
    );
}
/// decompile in the explicit form, recompile, and assert the normalized graphs match. node
/// order carries no execution meaning, so this compares the node *set* like the unordered helper.
fn assert_round_trips_explicit(src: &str) -> String {
    let first = compile(src);
    let rexrap = decompile_with(&first, &DecompileOptions { explicit: true }).expect("decompile");
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- explicit ---\n{rexrap}"));

    let sorted_nodes = |definition: &runinator_models::workflows::WorkflowGraph| {
        let normalized = runinator_workflows::normalize_definition(definition.clone());
        let value = serde_json::to_value(&normalized).expect("serialize graph");
        let mut nodes = value
            .get("nodes")
            .and_then(|n| n.as_array())
            .cloned()
            .unwrap_or_default();
        nodes.sort_by(|a, b| {
            let id = |v: &serde_json::Value| {
                v.get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            id(a).cmp(&id(b))
        });
        (value.get("start").cloned(), nodes)
    };

    assert_eq!(
        sorted_nodes(&first.definition),
        sorted_nodes(&second.definition),
        "explicit round trip diverged\n--- explicit ---\n{rexrap}"
    );
    rexrap
}
