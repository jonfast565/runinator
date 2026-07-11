use std::collections::{BTreeMap, BTreeSet};

use runinator_models::{
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata, RuninatorType},
    settings::{SettingKind, SettingSummary},
    types::RuninatorField,
    value::Value,
};
use serde::{Deserialize, Serialize};

use crate::{
    ast::{Block, Expr, ExprKind, PathSeg, Stmt, StmtKind},
    lower::types::lower_type,
    parse_document,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WdlCompletionRequest {
    pub source: String,
    pub cursor_byte: usize,
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
    // known config/secret slots, used to complete `config.scope.name` / `secret.scope.name`.
    #[serde(default)]
    pub settings: Vec<SettingSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WdlCompletionResponse {
    pub replace_start_byte: usize,
    pub replace_end_byte: usize,
    pub items: Vec<WdlCompletionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WdlCompletionItem {
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    pub insert_text: String,
    pub is_snippet: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CompletionContext {
    pub(crate) input: RuninatorType,
    pub(crate) bindings: BTreeMap<String, RuninatorType>,
    pub(crate) scoped: BTreeMap<String, RuninatorType>,
    pub(crate) labels: BTreeSet<String>,
    // namespace scope derived from the document's `import`s and `fn` definitions, mirroring
    // namespace resolution so bare/aliased completions only offer in-scope names.
    pub(crate) namespace: NamespaceScope,
}

/// the names a bare or aliased call may resolve to, gathered from imports and user functions.
#[derive(Debug, Clone, Default)]
pub(crate) struct NamespaceScope {
    /// import alias -> the std module it targets (e.g. `s` -> `strings`). non-std aliases are
    /// omitted because their namespaces have no completable compute members.
    pub(crate) aliases: BTreeMap<String, String>,
    /// intrinsic leaves callable bare because their std module was imported unaliased.
    pub(crate) bare_intrinsics: BTreeSet<String>,
    /// user-defined function names, always callable bare.
    pub(crate) user_fns: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ActionCallContext {
    provider: String,
    action: String,
    replace_start: usize,
    replace_end: usize,
    used_args: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct CompletionSpanContext {
    replace_start: usize,
}

/// complete wdl at a byte cursor using provider metadata and local type context.
pub fn complete_source(request: WdlCompletionRequest) -> WdlCompletionResponse {
    let source = request.source;
    let cursor = clamp_to_char_boundary(&source, request.cursor_byte);
    let word = current_word(&source, cursor);

    if !is_completion_allowed(&source, cursor) {
        return empty_response(word.start, cursor);
    }

    if let Some(path) = path_context(&source, cursor)
        && path.head == "std"
    {
        return complete_std_path(path);
    }

    if let Some(action) = action_member_context(&source, cursor)
        && find_provider(&request.providers, &action.provider).is_some()
    {
        return complete_actions(
            &request.providers,
            action.provider,
            action.replace_start,
            cursor,
        );
    }

    let context = completion_context(&source, cursor, &request.providers);
    if let Some(target) = transition_target_context(&source, cursor) {
        return complete_transition_targets(&context, target.replace_start, cursor);
    }
    if let Some(edge) = edge_outcome_context(&source, cursor) {
        return complete_edge_outcomes(edge.replace_start, cursor);
    }
    if let Some(path) = path_context(&source, cursor) {
        if path.head == "config" || path.head == "secret" {
            return complete_setting_path(&request.settings, path);
        }
        if path.head == "std" {
            return complete_std_path(path);
        }
        if let Some(module) = context.namespace.aliases.get(&path.head) {
            return complete_alias_path(&module.clone(), path);
        }
        if let Some(response) = complete_path(path, &context) {
            return response;
        }
    }

    if let Some(call) = action_call_context(&source, cursor) {
        return complete_action_args(&request.providers, call);
    }

    complete_bare(&request.providers, &context, word.start, cursor)
}

fn empty_response(replace_start_byte: usize, replace_end_byte: usize) -> WdlCompletionResponse {
    WdlCompletionResponse {
        replace_start_byte,
        replace_end_byte,
        items: Vec::new(),
    }
}

// complete a bare word: providers (for action positions) plus the in-scope bare names a namespaced
// program can call without qualification (user functions and unaliased-imported intrinsics).
fn complete_bare(
    providers: &[ProviderMetadata],
    context: &CompletionContext,
    replace_start: usize,
    replace_end: usize,
) -> WdlCompletionResponse {
    let mut items = construct_completion_items();
    items.extend(providers.iter().map(|provider| WdlCompletionItem {
        label: provider.name.clone(),
        kind: "provider".into(),
        detail: Some("provider".into()),
        documentation: None,
        insert_text: provider.name.clone(),
        is_snippet: false,
    }));
    for name in &context.namespace.user_fns {
        items.push(WdlCompletionItem {
            label: name.clone(),
            kind: "function".into(),
            detail: Some("function".into()),
            documentation: None,
            insert_text: name.clone(),
            is_snippet: false,
        });
    }
    for leaf in &context.namespace.bare_intrinsics {
        let detail = runinator_workflows::intrinsic_module(leaf)
            .map(|module| format!("std.{module}.{leaf}"))
            .unwrap_or_else(|| "std".into());
        items.push(WdlCompletionItem {
            label: leaf.clone(),
            kind: "function".into(),
            detail: Some(detail),
            documentation: None,
            insert_text: leaf.clone(),
            is_snippet: false,
        });
    }
    for label in &context.labels {
        items.push(WdlCompletionItem {
            label: label.clone(),
            kind: "node".into(),
            detail: Some("node".into()),
            documentation: None,
            insert_text: label.clone(),
            is_snippet: false,
        });
    }
    for name in context.scoped.keys() {
        items.push(WdlCompletionItem {
            label: name.clone(),
            kind: "local".into(),
            detail: Some("local".into()),
            documentation: None,
            insert_text: name.clone(),
            is_snippet: false,
        });
    }
    dedupe_completion_items(&mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn dedupe_completion_items(items: &mut Vec<WdlCompletionItem>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.label.clone()));
}

fn complete_edge_outcomes(replace_start: usize, replace_end: usize) -> WdlCompletionResponse {
    let mut items = [
        ("ok", "success edge", "ok -> ${target}"),
        ("fail", "failure edge", "fail -> ${target}"),
        ("timeout", "timeout edge", "timeout -> ${target}"),
        ("reject", "approval rejection edge", "reject -> ${target}"),
        ("next", "next edge", "next -> ${target}"),
        ("when", "predicate edge", "when ${condition} -> ${target}"),
    ]
    .into_iter()
    .map(|(label, detail, insert_text)| WdlCompletionItem {
        label: label.into(),
        kind: "edge".into(),
        detail: Some(detail.into()),
        documentation: None,
        insert_text: insert_text.into(),
        is_snippet: true,
    })
    .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn complete_transition_targets(
    context: &CompletionContext,
    replace_start: usize,
    replace_end: usize,
) -> WdlCompletionResponse {
    let mut items = vec![
        WdlCompletionItem {
            label: "done".into(),
            kind: "target".into(),
            detail: Some("terminal target".into()),
            documentation: None,
            insert_text: "done".into(),
            is_snippet: false,
        },
        WdlCompletionItem {
            label: "fail".into(),
            kind: "target".into(),
            detail: Some("terminal target".into()),
            documentation: None,
            insert_text: "fail".into(),
            is_snippet: false,
        },
    ];
    items.extend(context.labels.iter().map(|label| WdlCompletionItem {
        label: label.clone(),
        kind: "node".into(),
        detail: Some("node target".into()),
        documentation: None,
        insert_text: label.clone(),
        is_snippet: false,
    }));
    dedupe_completion_items(&mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn construct_completion_items() -> Vec<WdlCompletionItem> {
    const CONSTRUCTS: &[(&str, &str, &str, &str, bool)] = &[
        (
            "workflow",
            "keyword",
            "workflow scaffold",
            "workflow \"${name}\" {\n    params {\n        ${}\n    }\n\n    ${}\n}",
            true,
        ),
        (
            "node",
            "keyword",
            "provider action node",
            "node ${name} <- ${provider}.${action}(${args})",
            true,
        ),
        (
            "compute",
            "keyword",
            "compute block",
            "node ${name} <- compute {\n    return ${value}\n}",
            true,
        ),
        (
            "if",
            "keyword",
            "conditional block",
            "if ${condition} {\n    ${}\n}",
            true,
        ),
        (
            "for",
            "keyword",
            "for loop",
            "for ${item} in ${collection} {\n    ${}\n}",
            true,
        ),
        (
            "while",
            "keyword",
            "while loop",
            "while ${condition} {\n    ${}\n}",
            true,
        ),
        (
            "match",
            "keyword",
            "match block",
            "match ${value} {\n    ${case} -> {\n        ${}\n    }\n}",
            true,
        ),
        (
            "toggle",
            "keyword",
            "toggle on/off switch",
            "toggle ${value} {\n    on -> {\n        ${}\n    }\n    off -> {\n        ${}\n    }\n}",
            true,
        ),
        (
            "split",
            "keyword",
            "percentage split",
            "split on ${key} {\n    ${50}% -> {\n        ${}\n    }\n    else -> {\n        ${}\n    }\n}",
            true,
        ),
        (
            "parallel",
            "keyword",
            "parallel branches",
            "parallel {\n    branch ${name} {\n        ${}\n    }\n}",
            true,
        ),
        (
            "try",
            "keyword",
            "try/catch block",
            "try {\n    ${}\n} catch {\n    ${}\n}",
            true,
        ),
        (
            "subflow",
            "keyword",
            "subflow call",
            "node ${name} <- subflow(\"${workflow}\", params: {\n    ${}\n})",
            true,
        ),
        (
            "subflow-detached",
            "keyword",
            "detached subflow",
            "subflow(\"${workflow}\", params: {\n    ${}\n}, detached: true)",
            true,
        ),
        (
            "wait",
            "keyword",
            "wait node",
            "node ${name} <- wait ${duration}",
            true,
        ),
        (
            "emit",
            "keyword",
            "emit node",
            "emit \"${name}\" { ${key}: ${value} }",
            true,
        ),
        (
            "output",
            "keyword",
            "output block (event + artifacts)",
            "output {\n    emit \"${event_type}\" {}\n    ${name} = ${source}\n}",
            true,
        ),
        (
            "type",
            "type",
            "named struct type",
            "type ${Name} {\n    ${field}: ${type}\n}",
            true,
        ),
        (
            "fn",
            "function",
            "function definition",
            "fn ${name}(${arg}: ${type}) -> ${return_type} = ${value}",
            true,
        ),
        (
            "import std",
            "keyword",
            "standard-library import",
            "import std.${module}",
            true,
        ),
        (
            "trigger cron",
            "keyword",
            "cron trigger",
            "trigger cron \"${cron}\" with { ${} }",
            true,
        ),
        (
            "watch",
            "keyword",
            "workflow guard",
            "watch ${condition} -> ${target}",
            true,
        ),
        (
            "gate condition",
            "keyword",
            "condition gate",
            "gate condition when ${condition} every ${interval} timeout ${deadline}",
            true,
        ),
        (
            "signal",
            "keyword",
            "external signal wait",
            "signal \"${name}\" key ${correlation}",
            true,
        ),
        (
            "compensate",
            "keyword",
            "compensating action",
            "compensate ${provider}.${action}(${args})",
            true,
        ),
        (
            "assert",
            "keyword",
            "invariant assertions",
            "assert {\n    \"${name}\": ${condition}\n}",
            true,
        ),
        (
            "transform",
            "keyword",
            "data reshape bindings",
            "transform {\n    ${name} = ${expr}\n}",
            true,
        ),
        (
            "audit",
            "keyword",
            "compliance audit record",
            "audit action \"${action}\" actor ${actor}",
            true,
        ),
        (
            "checkpoint",
            "keyword",
            "named state snapshot",
            "checkpoint \"${name}\"",
            true,
        ),
        (
            "mutex",
            "keyword",
            "cross-run exclusive lock; brackets a critical section as a block",
            "mutex \"${name}\" {\n\t${body}\n}",
            true,
        ),
        (
            "throttle",
            "keyword",
            "cross-run rate limiter",
            "throttle \"${name}\" rate ${n} per ${window}",
            true,
        ),
        (
            "await",
            "keyword",
            "wait for other run(s)",
            "await ${run_ids} mode \"all\"",
            true,
        ),
        (
            "debounce",
            "keyword",
            "trailing-delay debounce",
            "debounce \"${name}\" delay ${delay}",
            true,
        ),
        (
            "collect",
            "keyword",
            "timed accumulator",
            "collect \"${name}\" max ${count} timeout ${deadline}",
            true,
        ),
        (
            "barrier",
            "keyword",
            "multi-run rendezvous",
            "barrier \"${name}\" count ${n} timeout ${deadline}",
            true,
        ),
        (
            "circuit_breaker",
            "keyword",
            "cross-run failure guard",
            "circuit_breaker \"${name}\" threshold ${n} window ${window} cooldown ${cooldown}",
            true,
        ),
        (
            "event_source",
            "keyword",
            "stream-driven iteration",
            "event_source type \"${event_type}\" max ${count} timeout ${deadline}",
            true,
        ),
    ];
    CONSTRUCTS
        .iter()
        .map(
            |(label, kind, detail, insert_text, is_snippet)| WdlCompletionItem {
                label: (*label).into(),
                kind: (*kind).into(),
                detail: Some((*detail).into()),
                documentation: None,
                insert_text: (*insert_text).into(),
                is_snippet: *is_snippet,
            },
        )
        .collect()
}

fn complete_actions(
    providers: &[ProviderMetadata],
    provider_name: String,
    replace_start: usize,
    replace_end: usize,
) -> WdlCompletionResponse {
    let Some(provider) = find_provider(providers, &provider_name) else {
        return empty_response(replace_start, replace_end);
    };
    let mut items = provider
        .actions
        .iter()
        .map(|action| WdlCompletionItem {
            label: action.function_name.clone(),
            kind: "function".into(),
            detail: Some(action_signature(action)),
            documentation: action.description.clone(),
            insert_text: action.function_name.clone(),
            is_snippet: false,
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn complete_action_args(
    providers: &[ProviderMetadata],
    call: ActionCallContext,
) -> WdlCompletionResponse {
    let Some(provider) = find_provider(providers, &call.provider) else {
        return empty_response(call.replace_start, call.replace_end);
    };
    let Some(action) = provider
        .actions
        .iter()
        .find(|action| action.function_name == call.action)
    else {
        return empty_response(call.replace_start, call.replace_end);
    };

    let mut items = action
        .parameters
        .iter()
        .filter(|parameter| !call.used_args.contains(&parameter.name))
        .map(|parameter| {
            let required = if parameter.required {
                "required"
            } else {
                "optional"
            };
            let (insert_text, is_snippet) = parameter_arg_insert(parameter);
            WdlCompletionItem {
                label: parameter.name.clone(),
                kind: "property".into(),
                detail: Some(format!("{required} {}", parameter.ty.describe())),
                documentation: parameter
                    .description
                    .clone()
                    .or_else(|| parameter.label.clone()),
                insert_text,
                is_snippet,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: call.replace_start,
        replace_end_byte: call.replace_end,
        items,
    }
}

// complete a `config.scope.name` / `secret.scope.name` reference from the known settings.
fn complete_setting_path(settings: &[SettingSummary], path: PathContext) -> WdlCompletionResponse {
    let kind = if path.head == "secret" {
        SettingKind::Secret
    } else {
        SettingKind::Config
    };
    let mut labels = BTreeSet::new();
    match path.completed.as_slice() {
        // `config.` / `secret.` -> suggest distinct scopes.
        [] => {
            for setting in settings.iter().filter(|setting| setting.kind == kind) {
                labels.insert(setting.scope.clone());
            }
        }
        // `config.scope.` / `secret.scope.` -> suggest names within the scope.
        [scope] => {
            for setting in settings
                .iter()
                .filter(|setting| setting.kind == kind && &setting.scope == scope)
            {
                labels.insert(setting.name.clone());
            }
        }
        // settings are flat scope/name pairs; deeper paths have no statically-known shape.
        _ => return empty_response(path.replace_start, path.replace_end),
    }
    let detail = if path.completed.is_empty() {
        format!("{} scope", kind.as_str())
    } else {
        format!("{} setting", kind.as_str())
    };
    let item_kind = if path.completed.is_empty() {
        "setting-scope"
    } else {
        "setting"
    };
    let items = labels
        .into_iter()
        .map(|label| WdlCompletionItem {
            label: label.clone(),
            kind: item_kind.into(),
            detail: Some(detail.clone()),
            documentation: None,
            insert_text: label,
            is_snippet: false,
        })
        .collect();
    WdlCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    }
}

// build a parameter argument insertion: `name: <typed-default>` with the value as an editable
// snippet field so accepting the completion yields a valid, pre-selected literal.
fn parameter_arg_insert(parameter: &ParameterMetadata) -> (String, bool) {
    let name = &parameter.name;
    if let Some(default) = &parameter.default_value
        && let Some(literal) = scalar_literal(default)
    {
        return (format!("{name}: ${{{literal}}}"), true);
    }
    let (prefix, field, suffix) = typed_placeholder(&parameter.ty);
    (format!("{name}: {prefix}${{{field}}}{suffix}"), true)
}

// render a scalar default as an inline literal, or none when it cannot live inside a snippet field.
fn scalar_literal(value: &Value) -> Option<String> {
    let literal = match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
        _ => return None,
    };
    // snippet fields are delimited by braces, so a literal containing one cannot be inlined.
    if literal.contains('{') || literal.contains('}') {
        return None;
    }
    Some(literal)
}

// a type-appropriate empty placeholder: surrounding literal syntax plus the editable field text.
fn typed_placeholder(ty: &RuninatorType) -> (&'static str, &'static str, &'static str) {
    match ty {
        RuninatorType::String => ("\"", "", "\""),
        RuninatorType::Integer | RuninatorType::Number => ("", "0", ""),
        RuninatorType::Boolean => ("", "false", ""),
        RuninatorType::Null => ("", "null", ""),
        RuninatorType::Array(_) => ("[", "", "]"),
        RuninatorType::Map(_) | RuninatorType::Struct { .. } => ("{", "", "}"),
        _ => ("", "", ""),
    }
}

// complete the builtin standard library: `std.` suggests modules, `std.<module>.` suggests the
// module's function leaves. driven by the shared module map so it never drifts from the runtime.
fn complete_std_path(path: PathContext) -> WdlCompletionResponse {
    let mut items = Vec::new();
    match path.completed.as_slice() {
        [] => {
            for module in runinator_workflows::STD_MODULES {
                items.push(WdlCompletionItem {
                    label: (*module).into(),
                    kind: "module".into(),
                    detail: Some("std module".into()),
                    documentation: None,
                    insert_text: (*module).into(),
                    is_snippet: false,
                });
            }
        }
        [module] => items.extend(module_leaf_items(module)),
        _ => {}
    }
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    }
}

// complete the leaves of a std module addressed through an import alias (`s.` -> strings leaves).
fn complete_alias_path(module: &str, path: PathContext) -> WdlCompletionResponse {
    // an alias binds a single module, so only the bare leaf is completable; deeper paths have none.
    let mut items = if path.completed.is_empty() {
        module_leaf_items(module)
    } else {
        Vec::new()
    };
    items.sort_by(|left, right| left.label.cmp(&right.label));
    WdlCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    }
}

// every intrinsic leaf name, across pure, effectful, and higher-order builtins.
fn intrinsic_leaf_names() -> impl Iterator<Item = &'static str> {
    runinator_workflows::PureIntrinsics::names()
        .iter()
        .chain(runinator_workflows::EFFECTFUL_INTRINSIC_NAMES.iter())
        .chain(runinator_workflows::HIGHER_ORDER_NAMES.iter())
        .copied()
}

// completion items for every intrinsic leaf in a std module, labelled by their qualified name.
fn module_leaf_items(module: &str) -> Vec<WdlCompletionItem> {
    intrinsic_leaf_names()
        .filter(|leaf| runinator_workflows::intrinsic_module(leaf) == Some(module))
        .map(|leaf| WdlCompletionItem {
            label: leaf.into(),
            kind: "function".into(),
            detail: Some(format!("std.{module}.{leaf}")),
            documentation: None,
            insert_text: leaf.into(),
            is_snippet: false,
        })
        .collect()
}

fn complete_path(path: PathContext, context: &CompletionContext) -> Option<WdlCompletionResponse> {
    let base = root_type(&path.head, context)?;
    let ty = navigate_type(base, &path.completed)?;
    let fields = type_fields(&ty)?;
    let mut items = fields
        .into_iter()
        .map(|(name, field)| WdlCompletionItem {
            label: name.clone(),
            kind: "property".into(),
            detail: Some(field.ty.describe().to_string()),
            documentation: if field.required {
                None
            } else {
                Some("optional".into())
            },
            insert_text: name,
            is_snippet: false,
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    Some(WdlCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    })
}

pub(crate) fn root_type(name: &str, context: &CompletionContext) -> Option<RuninatorType> {
    if name == "params" {
        return Some(context.input.clone());
    }
    if name == "run" {
        return Some(workflow_context_type());
    }
    // config and secret are opaque roots: recognized, but with no statically-known fields.
    if name == "config" || name == "secret" {
        return Some(RuninatorType::Any);
    }
    context
        .scoped
        .get(name)
        .or_else(|| context.bindings.get(name))
        .cloned()
}

pub(crate) fn type_fields(ty: &RuninatorType) -> Option<Vec<(String, RuninatorField)>> {
    match ty {
        RuninatorType::Struct { fields, .. } => Some(
            fields
                .iter()
                .map(|(key, field)| (key.clone(), field.clone()))
                .collect(),
        ),
        RuninatorType::Union(variants) => {
            let mut merged = BTreeMap::new();
            for variant in variants {
                if let RuninatorType::Struct { fields, .. } = variant {
                    for (key, field) in fields {
                        merged.entry(key.clone()).or_insert_with(|| field.clone());
                    }
                }
            }
            if merged.is_empty() {
                None
            } else {
                Some(merged.into_iter().collect())
            }
        }
        _ => None,
    }
}

pub(crate) fn navigate_type(mut ty: RuninatorType, segs: &[String]) -> Option<RuninatorType> {
    for seg in segs {
        ty = match ty {
            RuninatorType::Struct { fields, additional } => fields
                .get(seg)
                .map(|field| field.ty.clone())
                .or_else(|| additional.map(|extra| *extra))?,
            RuninatorType::Map(values) => *values,
            RuninatorType::Array(element) if seg.parse::<usize>().is_ok() => *element,
            RuninatorType::Union(variants) => {
                let mut matches = variants
                    .into_iter()
                    .filter_map(|variant| navigate_type(variant, std::slice::from_ref(seg)))
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    matches.remove(0)
                } else if matches.is_empty() {
                    return None;
                } else {
                    RuninatorType::Union(matches)
                }
            }
            _ => return None,
        };
    }
    Some(ty)
}

pub(crate) fn completion_context(
    source: &str,
    cursor: usize,
    providers: &[ProviderMetadata],
) -> CompletionContext {
    let document = parse_document(source).or_else(|_| {
        let mut patched = String::with_capacity(source.len() + "__wdl_completion__".len());
        patched.push_str(&source[..cursor]);
        patched.push_str("__wdl_completion__");
        patched.push_str(&source[cursor..]);
        parse_document(&patched)
    });
    let Ok(document) = document else {
        return CompletionContext::default();
    };
    let workflow = document.workflows.first();
    let input = workflow
        .and_then(|workflow| workflow.input.as_ref().and_then(|ty| lower_type(ty).ok()))
        .unwrap_or(RuninatorType::Any);
    let mut context = CompletionContext {
        input,
        labels: workflow
            .map(|workflow| collect_labels(&workflow.body))
            .unwrap_or_default(),
        namespace: collect_namespace_scope(&document),
        ..Default::default()
    };
    if let Some(workflow) = workflow {
        collect_block_context(&workflow.body, cursor, providers, &mut context);
    }
    context
}

pub(crate) fn collect_labels(block: &Block) -> BTreeSet<String> {
    let mut labels = BTreeSet::new();
    collect_block_labels(block, &mut labels);
    labels
}

fn collect_block_labels(block: &Block, labels: &mut BTreeSet<String>) {
    for stmt in block {
        if let Some(id) = stmt.annotations.id.as_deref().or(stmt.label.as_deref()) {
            labels.insert(id.to_string());
        }
        for child in completion_child_blocks(&stmt.kind) {
            collect_block_labels(child, labels);
        }
    }
}

fn completion_child_blocks(kind: &StmtKind) -> Vec<&Block> {
    match kind {
        StmtKind::If(if_stmt) => {
            let mut blocks: Vec<&Block> = if_stmt.arms.iter().map(|(_, body)| body).collect();
            if let Some(else_block) = &if_stmt.else_block {
                blocks.push(else_block);
            }
            blocks
        }
        StmtKind::For(for_stmt) => vec![&for_stmt.body],
        StmtKind::While(while_stmt) => vec![&while_stmt.body],
        StmtKind::Map(map_stmt) => vec![&map_stmt.body],
        StmtKind::Match(match_stmt) => {
            let mut blocks: Vec<&Block> = match_stmt.arms.iter().map(|arm| &arm.body).collect();
            if let Some(default) = &match_stmt.default {
                blocks.push(default);
            }
            blocks
        }
        StmtKind::Parallel(parallel) => parallel.branches.iter().collect(),
        StmtKind::Race(race) => race.branches.iter().collect(),
        StmtKind::Try(try_stmt) => {
            let mut blocks = vec![&try_stmt.body];
            if let Some(catch) = &try_stmt.catch {
                blocks.push(catch);
            }
            if let Some(finally) = &try_stmt.finally {
                blocks.push(finally);
            }
            blocks
        }
        _ => Vec::new(),
    }
}

// gather the bare/aliased names in scope from the document's imports and user functions, mirroring
// the namespace resolution pass so completion only offers names that resolve.
fn collect_namespace_scope(document: &crate::ast::Document) -> NamespaceScope {
    let mut scope = NamespaceScope {
        user_fns: document
            .functions
            .iter()
            .map(|function| function.name.clone())
            .collect(),
        ..Default::default()
    };
    for import in document
        .workflows
        .iter()
        .flat_map(|workflow| &workflow.imports)
    {
        let segments: Vec<&str> = import.path.split('.').collect();
        let is_std = segments.first() == Some(&runinator_workflows::STD_NAMESPACE);
        match (import.alias.as_deref(), segments.as_slice()) {
            // `import std` opens every intrinsic leaf into bare scope.
            (None, [ns]) if *ns == runinator_workflows::STD_NAMESPACE => {
                scope
                    .bare_intrinsics
                    .extend(intrinsic_leaf_names().map(str::to_string));
            }
            // `import std.<module>` opens that module's leaves into bare scope.
            (None, [_, module]) if is_std => {
                scope.bare_intrinsics.extend(
                    intrinsic_leaf_names()
                        .filter(|leaf| runinator_workflows::intrinsic_module(leaf) == Some(*module))
                        .map(str::to_string),
                );
            }
            // `import std.<module> as alias` binds the alias to a completable std module.
            (Some(alias), [_, module]) if is_std => {
                scope
                    .aliases
                    .insert(alias.to_string(), (*module).to_string());
            }
            // bare or aliased non-std imports name workflow namespaces with no compute members.
            _ => {}
        }
    }
    scope
}

fn collect_block_context(
    block: &Block,
    cursor: usize,
    providers: &[ProviderMetadata],
    context: &mut CompletionContext,
) {
    for stmt in block {
        if stmt.span.start <= cursor {
            record_statement_binding(stmt, providers, context);
        }
        if stmt.span.start <= cursor && cursor <= stmt.span.end {
            collect_child_context(stmt, cursor, providers, context);
        }
    }
}

fn collect_child_context(
    stmt: &Stmt,
    cursor: usize,
    providers: &[ProviderMetadata],
    context: &mut CompletionContext,
) {
    match &stmt.kind {
        StmtKind::For(for_stmt) => {
            if cursor <= for_stmt.items.span.end {
                collect_block_context(&for_stmt.body, cursor, providers, context);
                return;
            }
            let item_type = infer_expr_type(&for_stmt.items, context)
                .and_then(array_element_type)
                .unwrap_or(RuninatorType::Any);
            context.scoped.insert(for_stmt.var.clone(), item_type);
            collect_block_context(&for_stmt.body, cursor, providers, context);
        }
        StmtKind::Map(map_stmt) => {
            if cursor <= map_stmt.items.span.end {
                collect_block_context(&map_stmt.body, cursor, providers, context);
                return;
            }
            let item_type = infer_expr_type(&map_stmt.items, context)
                .and_then(array_element_type)
                .unwrap_or(RuninatorType::Any);
            context.scoped.insert(map_stmt.var.clone(), item_type);
            collect_block_context(&map_stmt.body, cursor, providers, context);
        }
        StmtKind::If(if_stmt) => {
            for (_, body) in &if_stmt.arms {
                collect_block_context(body, cursor, providers, context);
            }
            if let Some(body) = &if_stmt.else_block {
                collect_block_context(body, cursor, providers, context);
            }
        }
        StmtKind::Match(match_stmt) => {
            for arm in &match_stmt.arms {
                collect_block_context(&arm.body, cursor, providers, context);
            }
            if let Some(body) = &match_stmt.default {
                collect_block_context(body, cursor, providers, context);
            }
        }
        StmtKind::Parallel(parallel) => {
            for body in &parallel.branches {
                collect_block_context(body, cursor, providers, context);
            }
        }
        StmtKind::Race(race) => {
            for body in &race.branches {
                collect_block_context(body, cursor, providers, context);
            }
        }
        StmtKind::Try(try_stmt) => {
            collect_block_context(&try_stmt.body, cursor, providers, context);
            if let Some(body) = &try_stmt.catch {
                collect_block_context(body, cursor, providers, context);
            }
            if let Some(body) = &try_stmt.finally {
                collect_block_context(body, cursor, providers, context);
            }
        }
        _ => {}
    }
}

fn record_statement_binding(
    stmt: &Stmt,
    providers: &[ProviderMetadata],
    context: &mut CompletionContext,
) {
    let Some(id) = stmt.annotations.id.as_deref().or(stmt.label.as_deref()) else {
        return;
    };
    let ty = if let Some(label_type) = &stmt.label_type {
        lower_type(label_type).unwrap_or(RuninatorType::Any)
    } else {
        match &stmt.kind {
            StmtKind::Action(action) => {
                provider_action_output_type(providers, &action.provider, &action.function)
                    .unwrap_or(RuninatorType::Any)
            }
            StmtKind::Subflow(_) => subflow_output_type(),
            _ => RuninatorType::Any,
        }
    };
    context.bindings.insert(id.to_string(), ty);
}

pub(crate) fn provider_action_output_type(
    providers: &[ProviderMetadata],
    provider_name: &str,
    action_name: &str,
) -> Option<RuninatorType> {
    let action = find_provider(providers, provider_name)?
        .actions
        .iter()
        .find(|action| action.function_name == action_name)?;
    Some(action.results_type())
}

fn infer_expr_type(expr: &Expr, context: &CompletionContext) -> Option<RuninatorType> {
    match &expr.kind {
        ExprKind::Null => Some(RuninatorType::Null),
        ExprKind::Bool(_) => Some(RuninatorType::Boolean),
        ExprKind::Int(_) => Some(RuninatorType::Integer),
        ExprKind::Float(_) => Some(RuninatorType::Number),
        ExprKind::Str(_) => Some(RuninatorType::String),
        ExprKind::FileInclude { .. } => Some(RuninatorType::String),
        ExprKind::DirInclude { .. } => Some(RuninatorType::array(RuninatorType::String)),
        ExprKind::Compare { .. } => Some(RuninatorType::Boolean),
        ExprKind::Ternary { then, els, .. } => {
            let then_ty = infer_expr_type(then, context)?;
            (Some(&then_ty) == infer_expr_type(els, context).as_ref()).then_some(then_ty)
        }
        ExprKind::InlineCode { .. } => Some(RuninatorType::String),
        ExprKind::Array(items) => {
            let item_type = items
                .first()
                .and_then(|item| infer_expr_type(item, context))
                .unwrap_or(RuninatorType::Any);
            Some(RuninatorType::array(item_type))
        }
        ExprKind::Object(entries) => Some(RuninatorType::structure(entries.iter().filter_map(
            |(key, value)| infer_expr_type(value, context).map(|ty| (key.clone(), ty)),
        ))),
        ExprKind::Concat(_) | ExprKind::ToString(_) => Some(RuninatorType::String),
        ExprKind::Coalesce(items) => items
            .first()
            .and_then(|item| infer_expr_type(item, context)),
        ExprKind::ToJson(_) => Some(RuninatorType::String),
        ExprKind::Add(_)
        | ExprKind::Sub(_)
        | ExprKind::Mul(_)
        | ExprKind::Div(_)
        | ExprKind::Mod(_)
        | ExprKind::Neg(_) => Some(RuninatorType::Number),
        ExprKind::Call { .. } => Some(RuninatorType::Any),
        // a lambda carries no value type of its own.
        ExprKind::Lambda { .. } => None,
        ExprKind::Path(segs) => infer_path_type(segs, context),
        // a spread carries no value type of its own; it is resolved by desugaring.
        ExprKind::Spread(_) => None,
    }
}

fn infer_path_type(segs: &[PathSeg], context: &CompletionContext) -> Option<RuninatorType> {
    let Some(PathSeg::Key(head)) = segs.first() else {
        return None;
    };
    let root = root_type(head, context)?;
    let rest = segs[1..]
        .iter()
        .map(|seg| match seg {
            PathSeg::Key(key) => key.clone(),
            PathSeg::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>();
    navigate_type(root, &rest)
}

fn array_element_type(ty: RuninatorType) -> Option<RuninatorType> {
    match ty {
        RuninatorType::Array(element) => Some(*element),
        _ => None,
    }
}

pub(crate) fn workflow_context_type() -> RuninatorType {
    RuninatorType::structure([
        ("run_id", RuninatorType::Integer),
        ("workflow_id", RuninatorType::Integer),
        ("name", RuninatorType::String),
        ("state", RuninatorType::Any),
    ])
}

fn subflow_output_type() -> RuninatorType {
    RuninatorType::structure([
        ("subflow_run_id", RuninatorType::Integer),
        ("subflow_workflow_id", RuninatorType::Integer),
        ("run_name", RuninatorType::String),
        ("reused", RuninatorType::Boolean),
        ("status", RuninatorType::String),
        ("state", RuninatorType::Any),
        ("parameters", RuninatorType::Any),
    ])
}

pub(crate) fn find_provider<'a>(
    providers: &'a [ProviderMetadata],
    name: &str,
) -> Option<&'a ProviderMetadata> {
    providers
        .iter()
        .find(|provider| provider.name == name)
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.name.eq_ignore_ascii_case(name))
        })
}

pub(crate) fn action_signature(action: &ActionMetadata) -> String {
    let params = action
        .parameters
        .iter()
        .map(|parameter| {
            let suffix = if parameter.required { "" } else { "?" };
            format!("{}{}: {}", parameter.name, suffix, parameter.ty.describe())
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({params})")
}

fn transition_target_context(source: &str, cursor: usize) -> Option<CompletionSpanContext> {
    let word = current_word(source, cursor);
    let before_word = &source[..word.start];
    if before_word.trim_end().ends_with("->") || previous_word(before_word) == Some("goto") {
        return Some(CompletionSpanContext {
            replace_start: word.start,
        });
    }
    None
}

fn edge_outcome_context(source: &str, cursor: usize) -> Option<CompletionSpanContext> {
    let word = current_word(source, cursor);
    if transition_target_context(source, cursor).is_some() {
        return None;
    }
    if inside_edges_block(source, word.start) {
        return Some(CompletionSpanContext {
            replace_start: word.start,
        });
    }

    let line_start = source[..word.start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let prefix = source[line_start..word.start].trim_end();
    if prefix.is_empty() || prefix.ends_with("->") {
        return None;
    }
    let trimmed = prefix.trim_start();
    if trimmed.starts_with("node ") && completed_statement_prefix(trimmed) {
        return Some(CompletionSpanContext {
            replace_start: word.start,
        });
    }
    None
}

fn completed_statement_prefix(prefix: &str) -> bool {
    prefix.ends_with(')') || prefix.ends_with('}') || prefix.ends_with('"')
}

fn inside_edges_block(source: &str, cursor: usize) -> bool {
    let Some(edges_start) = source[..cursor].rfind("edges") else {
        return false;
    };
    if !is_keyword_at(source, edges_start, "edges") {
        return false;
    }
    let Some(open_rel) = source[edges_start..cursor].find('{') else {
        return false;
    };
    let open = edges_start + open_rel;
    let mut depth = 0usize;
    for byte in source[open..cursor].bytes() {
        match byte {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth > 0
}

fn is_keyword_at(source: &str, start: usize, keyword: &str) -> bool {
    let end = start + keyword.len();
    let before_ok = start == 0 || !is_ident_continue(source.as_bytes()[start - 1]);
    let after_ok = source
        .as_bytes()
        .get(end)
        .is_none_or(|byte| !is_ident_continue(*byte));
    before_ok && after_ok
}

fn previous_word(source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut end = source.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_continue(bytes[start - 1]) {
        start -= 1;
    }
    (start < end).then_some(&source[start..end])
}

#[derive(Debug, Clone)]
pub(crate) struct PathContext {
    pub(crate) head: String,
    pub(crate) completed: Vec<String>,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
}

pub(crate) fn path_context(source: &str, cursor: usize) -> Option<PathContext> {
    let (start, end) = current_path_bounds(source, cursor);
    if start == end {
        return None;
    }
    let token = &source[start..end];
    if !token.contains('.') {
        return None;
    }
    let mut parts = token.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts[0].is_empty() {
        return None;
    }
    let partial = parts.pop().unwrap_or_default();
    let completed = parts
        .iter()
        .skip(1)
        .filter(|part| !part.is_empty())
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    let replace_start = end.saturating_sub(partial.len());
    Some(PathContext {
        head: parts[0].to_string(),
        completed,
        replace_start,
        replace_end: cursor,
    })
}

#[derive(Debug, Clone)]
struct ActionMemberContext {
    provider: String,
    replace_start: usize,
}

fn action_member_context(source: &str, cursor: usize) -> Option<ActionMemberContext> {
    let word_start = current_word(source, cursor).start;
    let dot = previous_non_space(source, word_start)?;
    if source.as_bytes().get(dot) != Some(&b'.') {
        return None;
    }
    let provider_end = dot;
    let provider_start = identifier_start_before(source, provider_end)?;
    if provider_start > 0 && source.as_bytes().get(provider_start - 1) == Some(&b'.') {
        return None;
    }
    let provider = source[provider_start..provider_end].to_string();
    Some(ActionMemberContext {
        provider,
        replace_start: word_start,
    })
}

fn action_call_context(source: &str, cursor: usize) -> Option<ActionCallContext> {
    let open = unmatched_open_paren(source, cursor)?;
    let before_open = source[..open].trim_end();
    let dot = before_open.rfind('.')?;
    let action_start = identifier_start_before(before_open, before_open.len())?;
    if action_start <= dot {
        return None;
    }
    let provider_end = dot;
    let provider_start = identifier_start_before(before_open, provider_end)?;
    let provider = before_open[provider_start..provider_end].to_string();
    let action = before_open[action_start..before_open.len()].to_string();
    if provider.is_empty() || action.is_empty() {
        return None;
    }
    let word = current_word(source, cursor);
    let used_args = used_argument_names(&source[open + 1..cursor]);
    Some(ActionCallContext {
        provider,
        action,
        replace_start: word.start,
        replace_end: cursor,
        used_args,
    })
}

fn used_argument_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                index += 1;
                continue;
            }
            b'(' | b'{' | b'[' => {
                depth += 1;
                index += 1;
                continue;
            }
            b')' | b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
                continue;
            }
            _ => {}
        }
        if depth == 0 && is_ident_start(byte) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            let mut lookahead = index;
            while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                lookahead += 1;
            }
            if bytes.get(lookahead) == Some(&b':') {
                names.insert(text[start..index].to_string());
            }
        } else {
            index += 1;
        }
    }
    names
}

pub(crate) fn unmatched_open_paren(source: &str, cursor: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in source[..cursor].char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' if depth == 0 => return Some(index),
            '(' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct WordBounds {
    start: usize,
}

fn current_word(source: &str, cursor: usize) -> WordBounds {
    let mut start = cursor;
    while start > 0 {
        let byte = source.as_bytes()[start - 1];
        if !is_ident_continue(byte) {
            break;
        }
        start -= 1;
    }
    WordBounds { start }
}

fn current_path_bounds(source: &str, cursor: usize) -> (usize, usize) {
    let mut start = cursor;
    while start > 0 {
        let byte = source.as_bytes()[start - 1];
        if !(is_ident_continue(byte) || byte == b'.') {
            break;
        }
        start -= 1;
    }
    (start, cursor)
}

fn previous_non_space(source: &str, cursor: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = cursor;
    while index > 0 {
        index -= 1;
        if !bytes[index].is_ascii_whitespace() {
            return Some(index);
        }
    }
    None
}

fn identifier_start_before(source: &str, end: usize) -> Option<usize> {
    if end == 0 {
        return None;
    }
    let bytes = source.as_bytes();
    let mut start = end;
    while start > 0 && is_action_ident_continue(bytes[start - 1]) {
        start -= 1;
    }
    if start == end { None } else { Some(start) }
}

pub(crate) fn clamp_to_char_boundary(source: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(source.len());
    while cursor > 0 && !source.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn is_completion_allowed(source: &str, cursor: usize) -> bool {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        LineComment,
        BlockComment,
        String,
        Interpolation(usize),
    }

    let bytes = source.as_bytes();
    let mut state = State::Normal;
    let mut index = 0;
    let mut escaped = false;
    while index < cursor {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Normal => {
                if byte == b'/' && next == Some(b'/') {
                    state = State::LineComment;
                    index += 2;
                    continue;
                }
                if byte == b'/' && next == Some(b'*') {
                    state = State::BlockComment;
                    index += 2;
                    continue;
                }
                if byte == b'"' {
                    state = State::String;
                    escaped = false;
                }
            }
            State::LineComment => {
                if byte == b'\n' {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if byte == b'*' && next == Some(b'/') {
                    state = State::Normal;
                    index += 2;
                    continue;
                }
            }
            State::String => {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'$' && next == Some(b'{') {
                    state = State::Interpolation(1);
                    index += 2;
                    continue;
                } else if byte == b'"' {
                    state = State::Normal;
                }
            }
            State::Interpolation(depth) => {
                if byte == b'{' {
                    state = State::Interpolation(depth + 1);
                } else if byte == b'}' {
                    if depth <= 1 {
                        state = State::String;
                    } else {
                        state = State::Interpolation(depth - 1);
                    }
                }
            }
        }
        index += 1;
    }
    matches!(state, State::Normal | State::Interpolation(_))
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_action_ident_continue(byte: u8) -> bool {
    is_ident_continue(byte) || byte == b'-'
}
