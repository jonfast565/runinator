use std::collections::{BTreeMap, BTreeSet};

use runinator_models::{
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata, RuninatorType},
    settings::{SettingKind, SettingSummary},
    types::RuninatorField,
    value::Value,
};
use serde::{Deserialize, Serialize};

use runinator_rexrap::{
    analysis::{STATEMENT_KEYWORDS, lower_type},
    ast::{Block, Expr, ExprKind, PathSeg, Stmt, StmtKind},
    parse_document,
};

use crate::cursor::{Cursor, clamp_to_char_boundary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RexRapCompletionRequest {
    pub source: String,
    pub cursor_byte: usize,
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
    // known config/secret slots, used to complete `config.scope.name` / `secret.scope.name`.
    #[serde(default)]
    pub settings: Vec<SettingSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RexRapCompletionResponse {
    pub replace_start_byte: usize,
    pub replace_end_byte: usize,
    pub items: Vec<RexRapCompletionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RexRapCompletionItem {
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
    // best-effort output type of the source-order predecessor node, used to type `prev`. `Any`
    // at ambiguous positions (first node, after a control-flow block, inside a nested block).
    pub(crate) prev: RuninatorType,
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
pub(crate) struct ActionCallContext {
    pub(crate) provider: String,
    pub(crate) action: String,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
    pub(crate) used_args: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionSpanContext {
    pub(crate) replace_start: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionMemberContext {
    pub(crate) provider: String,
    pub(crate) replace_start: usize,
}

/// complete rexrap at a byte cursor using provider metadata and local type context.
pub fn complete_source(request: RexRapCompletionRequest) -> RexRapCompletionResponse {
    let source = request.source;
    let pos = clamp_to_char_boundary(&source, request.cursor_byte);
    let cursor = Cursor::new(&source, pos);
    let word = cursor.current_word();

    if !cursor.is_completion_allowed() {
        return empty_response(word.start, pos);
    }

    if let Some(path) = cursor.path_context()
        && path.head == "std"
    {
        return complete_std_path(path);
    }

    if let Some(action) = cursor.action_member_context()
        && find_provider(&request.providers, &action.provider).is_some()
    {
        return complete_actions(
            &request.providers,
            action.provider,
            action.replace_start,
            pos,
        );
    }

    let context = completion_context(&source, pos, &request.providers);
    if let Some(target) = cursor.transition_target_context() {
        return complete_transition_targets(&context, target.replace_start, pos);
    }
    if let Some(edge) = cursor.edge_outcome_context() {
        return complete_edge_outcomes(edge.replace_start, pos);
    }
    if let Some(path) = cursor.path_context() {
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

    if let Some(call) = cursor.action_call_context() {
        return complete_action_args(&request.providers, call);
    }

    complete_bare(&request.providers, &context, word.start, pos)
}

fn empty_response(replace_start_byte: usize, replace_end_byte: usize) -> RexRapCompletionResponse {
    RexRapCompletionResponse {
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
) -> RexRapCompletionResponse {
    let mut items = construct_completion_items();
    items.extend(providers.iter().map(|provider| RexRapCompletionItem {
        label: provider.name.clone(),
        kind: "provider".into(),
        detail: Some("provider".into()),
        documentation: None,
        insert_text: provider.name.clone(),
        is_snippet: false,
    }));
    for name in &context.namespace.user_fns {
        items.push(RexRapCompletionItem {
            label: name.clone(),
            kind: "function".into(),
            detail: Some("function".into()),
            documentation: None,
            insert_text: name.clone(),
            is_snippet: false,
        });
    }
    for leaf in &context.namespace.bare_intrinsics {
        let detail = runinator_compute::intrinsic_module(leaf)
            .map(|module| format!("std.{module}.{leaf}"))
            .unwrap_or_else(|| "std".into());
        items.push(RexRapCompletionItem {
            label: leaf.clone(),
            kind: "function".into(),
            detail: Some(detail),
            documentation: None,
            insert_text: leaf.clone(),
            is_snippet: false,
        });
    }
    for label in &context.labels {
        items.push(RexRapCompletionItem {
            label: label.clone(),
            kind: "node".into(),
            detail: Some("node".into()),
            documentation: None,
            insert_text: label.clone(),
            is_snippet: false,
        });
    }
    for (name, ty) in &context.scoped {
        items.push(RexRapCompletionItem {
            label: name.clone(),
            kind: "local".into(),
            detail: Some(format!("local: {}", ty.describe())),
            documentation: None,
            insert_text: name.clone(),
            is_snippet: false,
        });
    }
    dedupe_completion_items(&mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    RexRapCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn dedupe_completion_items(items: &mut Vec<RexRapCompletionItem>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.label.clone()));
}

fn complete_edge_outcomes(replace_start: usize, replace_end: usize) -> RexRapCompletionResponse {
    let mut items = [
        (
            "on success",
            "success route",
            "on success {\n    continue ${target}\n}",
        ),
        (
            "on failure",
            "failure route",
            "on failure {\n    continue ${target}\n}",
        ),
        (
            "on timeout",
            "timeout route",
            "on timeout {\n    continue ${target}\n}",
        ),
        (
            "on reject",
            "approval rejection route",
            "on reject {\n    continue ${target}\n}",
        ),
        (
            "on next",
            "next route",
            "on next {\n    continue ${target}\n}",
        ),
        (
            "when",
            "predicate route",
            "when ${condition} {\n    continue ${target}\n}",
        ),
    ]
    .into_iter()
    .map(|(label, detail, insert_text)| RexRapCompletionItem {
        label: label.into(),
        kind: "route".into(),
        detail: Some(detail.into()),
        documentation: None,
        insert_text: insert_text.into(),
        is_snippet: true,
    })
    .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    RexRapCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn complete_transition_targets(
    context: &CompletionContext,
    replace_start: usize,
    replace_end: usize,
) -> RexRapCompletionResponse {
    let mut items = vec![
        RexRapCompletionItem {
            label: "end".into(),
            kind: "target".into(),
            detail: Some("terminal target".into()),
            documentation: None,
            insert_text: "end".into(),
            is_snippet: false,
        },
        RexRapCompletionItem {
            label: "fail".into(),
            kind: "target".into(),
            detail: Some("terminal target".into()),
            documentation: None,
            insert_text: "fail".into(),
            is_snippet: false,
        },
    ];
    items.extend(context.labels.iter().map(|label| RexRapCompletionItem {
        label: label.clone(),
        kind: "node".into(),
        detail: Some("node target".into()),
        documentation: None,
        insert_text: label.clone(),
        is_snippet: false,
    }));
    dedupe_completion_items(&mut items);
    items.sort_by(|left, right| left.label.cmp(&right.label));
    RexRapCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn construct_completion_items() -> Vec<RexRapCompletionItem> {
    const CONSTRUCTS: &[(&str, &str, &str, &str, bool)] = &[
        (
            "workflow",
            "keyword",
            "workflow scaffold",
            "workflow \"${name}\" {\n    params {\n        ${}\n    }\n\n    do {\n        ${}\n    }\n}",
            true,
        ),
        (
            "let",
            "keyword",
            "provider action node",
            "let ${name} = ${provider}.${action}(${args})",
            true,
        ),
        ("do", "keyword", "runtime block", "do {\n    ${}\n}", true),
        (
            "routes",
            "keyword",
            "attached routes section",
            "routes {\n    on success {\n        continue ${target}\n    }\n}",
            true,
        ),
        (
            "join",
            "keyword",
            "named continuation",
            "join ${name} {\n    ${}\n}",
            true,
        ),
        (
            "async",
            "keyword",
            "schedule a call as a task",
            "let ${name} = async ${provider}.${action}(${args})",
            true,
        ),
        (
            "await",
            "keyword",
            "join a task handle",
            "let ${name} = await ${handle}",
            true,
        ),
        (
            "detach",
            "keyword",
            "drop a task handle without joining it",
            "detach ${handle}",
            true,
        ),
        (
            "return",
            "keyword",
            "successful terminal with a result",
            "return ${value}",
            true,
        ),
        (
            "task fn",
            "keyword",
            "runtime function inlined at each call site",
            "task fn ${name}(${params}) do {\n    ${}\n}",
            true,
        ),
        (
            "compute",
            "keyword",
            "compute block",
            "let ${name} = compute {\n    return ${value}\n}",
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
            "map",
            "keyword",
            "concurrent map",
            "map ${item} in ${collection} concurrency ${limit} {\n    ${}\n}",
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
            "let ${name} = subflow(\"${workflow}\", params: {\n    ${}\n})",
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
            "let ${name} = wait ${duration}",
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
            "trigger on_success",
            "keyword",
            "chained trigger",
            "trigger on_success workflow \"${target}\"",
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
            "gate condition when ${condition} every ${interval} timeout ${deadline} on_timeout ${policy}",
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
            "interrupt",
            "keyword",
            "interrupt handler region",
            "interrupt on wake {\n    ${}\n    resume\n}",
            true,
        ),
        (
            "resume",
            "keyword",
            "return control from an interrupt handler",
            "resume",
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
            "cooldown",
            "keyword",
            "cross-run cooldown; one pass per window",
            "cooldown \"${name}\" every ${window}",
            true,
        ),
        (
            "await",
            "keyword",
            "wait for run(s) of a named workflow",
            "await workflow \"${name}\" key ${correlation} mode \"all\"",
            true,
        ),
        (
            "correlate",
            "keyword",
            "declare this run's correlation key",
            "correlate key ${expr}",
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
    let mut items = CONSTRUCTS
        .iter()
        .map(
            |(label, kind, detail, insert_text, is_snippet)| RexRapCompletionItem {
                label: (*label).into(),
                kind: (*kind).into(),
                detail: Some((*detail).into()),
                documentation: None,
                insert_text: (*insert_text).into(),
                is_snippet: *is_snippet,
            },
        )
        .collect::<Vec<_>>();
    items.extend(
        STATEMENT_KEYWORDS
            .iter()
            .map(|keyword| RexRapCompletionItem {
                label: (*keyword).into(),
                kind: "keyword".into(),
                detail: Some("REXRAP statement".into()),
                documentation: None,
                insert_text: (*keyword).into(),
                is_snippet: false,
            }),
    );
    items
}

fn complete_actions(
    providers: &[ProviderMetadata],
    provider_name: String,
    replace_start: usize,
    replace_end: usize,
) -> RexRapCompletionResponse {
    let Some(provider) = find_provider(providers, &provider_name) else {
        return empty_response(replace_start, replace_end);
    };
    let mut items = provider
        .actions
        .iter()
        .map(|action| RexRapCompletionItem {
            label: action.function_name.clone(),
            kind: "function".into(),
            detail: Some(action_signature(action)),
            documentation: action.description.clone(),
            insert_text: action.function_name.clone(),
            is_snippet: false,
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    RexRapCompletionResponse {
        replace_start_byte: replace_start,
        replace_end_byte: replace_end,
        items,
    }
}

fn complete_action_args(
    providers: &[ProviderMetadata],
    call: ActionCallContext,
) -> RexRapCompletionResponse {
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
            RexRapCompletionItem {
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
    RexRapCompletionResponse {
        replace_start_byte: call.replace_start,
        replace_end_byte: call.replace_end,
        items,
    }
}

// complete a `config.scope.name` / `secret.scope.name` reference from the known settings.
fn complete_setting_path(
    settings: &[SettingSummary],
    path: PathContext,
) -> RexRapCompletionResponse {
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
        .map(|label| RexRapCompletionItem {
            label: label.clone(),
            kind: item_kind.into(),
            detail: Some(detail.clone()),
            documentation: None,
            insert_text: label,
            is_snippet: false,
        })
        .collect();
    RexRapCompletionResponse {
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
fn complete_std_path(path: PathContext) -> RexRapCompletionResponse {
    let mut items = Vec::new();
    match path.completed.as_slice() {
        [] => {
            for module in runinator_compute::STD_MODULES {
                items.push(RexRapCompletionItem {
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
    RexRapCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    }
}

// complete the leaves of a std module addressed through an import alias (`s.` -> strings leaves).
fn complete_alias_path(module: &str, path: PathContext) -> RexRapCompletionResponse {
    // an alias binds a single module, so only the bare leaf is completable; deeper paths have none.
    let mut items = if path.completed.is_empty() {
        module_leaf_items(module)
    } else {
        Vec::new()
    };
    items.sort_by(|left, right| left.label.cmp(&right.label));
    RexRapCompletionResponse {
        replace_start_byte: path.replace_start,
        replace_end_byte: path.replace_end,
        items,
    }
}

// every intrinsic leaf name, across pure, effectful, and higher-order builtins.
fn intrinsic_leaf_names() -> impl Iterator<Item = &'static str> {
    runinator_compute::PureIntrinsics::names()
        .iter()
        .chain(runinator_compute::EFFECTFUL_INTRINSIC_NAMES.iter())
        .chain(runinator_compute::HIGHER_ORDER_NAMES.iter())
        .copied()
}

// completion items for every intrinsic leaf in a std module, labelled by their qualified name.
fn module_leaf_items(module: &str) -> Vec<RexRapCompletionItem> {
    intrinsic_leaf_names()
        .filter(|leaf| runinator_compute::intrinsic_module(leaf) == Some(module))
        .map(|leaf| RexRapCompletionItem {
            label: leaf.into(),
            kind: "function".into(),
            detail: Some(format!("std.{module}.{leaf}")),
            documentation: None,
            insert_text: leaf.into(),
            is_snippet: false,
        })
        .collect()
}

fn complete_path(
    path: PathContext,
    context: &CompletionContext,
) -> Option<RexRapCompletionResponse> {
    let base = root_type(&path.head, context)?;
    let ty = navigate_type(base, &path.completed)?;
    let fields = type_fields(&ty)?;
    let mut items = fields
        .into_iter()
        .map(|(name, field)| RexRapCompletionItem {
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
    Some(RexRapCompletionResponse {
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
    // `prev` resolves to the source-order predecessor's output type when it is a producing node.
    if name == "prev" {
        return Some(context.prev.clone());
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
        let mut patched = String::with_capacity(source.len() + "__rexrap_completion__".len());
        patched.push_str(&source[..cursor]);
        patched.push_str("__rexrap_completion__");
        patched.push_str(&source[cursor..]);
        parse_document(&patched)
    });
    let Ok(mut document) = document else {
        return CompletionContext::default();
    };
    // best-effort: rewrite namespaced/aliased calls to their bare runtime form so expression-type
    // inference (intrinsic and higher-order result typing) sees the same names sema does. errors
    // (e.g. an unknown module mid-edit) leave the partially-resolved document, which is fine here.
    let _ = runinator_rexrap::analysis::resolve_namespaces(&mut document);
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
        StmtKind::Parallel(parallel) => parallel
            .branches
            .iter()
            .map(|branch| &branch.body)
            .collect(),
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
fn collect_namespace_scope(document: &runinator_rexrap::ast::Document) -> NamespaceScope {
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
        let is_std = segments.first() == Some(&runinator_compute::STD_NAMESPACE);
        match (import.alias.as_deref(), segments.as_slice()) {
            // `import std` opens every intrinsic leaf into bare scope.
            (None, [ns]) if *ns == runinator_compute::STD_NAMESPACE => {
                scope
                    .bare_intrinsics
                    .extend(intrinsic_leaf_names().map(str::to_string));
            }
            // `import std.<module>` opens that module's leaves into bare scope.
            (None, [_, module]) if is_std => {
                scope.bare_intrinsics.extend(
                    intrinsic_leaf_names()
                        .filter(|leaf| runinator_compute::intrinsic_module(leaf) == Some(*module))
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
    // a nested block inherits the predecessor visible where it was entered, then advances `prev`
    // across its own straight-line siblings.
    let mut prev = context.prev.clone();
    for stmt in block {
        if stmt.span.start <= cursor {
            record_statement_binding(stmt, providers, context);
        }
        if stmt.span.start <= cursor && cursor <= stmt.span.end {
            // the cursor is inside this statement: `prev` is the previous sibling's output. set it
            // before descending so a nested block can still override with its own local predecessor.
            context.prev = prev.clone();
            collect_child_context(stmt, cursor, providers, context);
        }
        if stmt.span.end < cursor {
            prev = statement_output_type(stmt, providers);
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
            let item_type = for_stmt
                .var_type
                .as_ref()
                .and_then(|ty| lower_type(ty).ok())
                .or_else(|| {
                    infer_expr_type(&for_stmt.items, context)
                        .and_then(|ty| higher_order_item_type(&ty))
                })
                .unwrap_or(RuninatorType::Any);
            context.scoped.insert(for_stmt.var.clone(), item_type);
            if let Some(index_var) = &for_stmt.index_var {
                context
                    .scoped
                    .insert(index_var.clone(), RuninatorType::Integer);
            }
            collect_block_context(&for_stmt.body, cursor, providers, context);
        }
        StmtKind::Map(map_stmt) => {
            if cursor <= map_stmt.items.span.end {
                collect_block_context(&map_stmt.body, cursor, providers, context);
                return;
            }
            let item_type = infer_expr_type(&map_stmt.items, context)
                .and_then(|ty| higher_order_item_type(&ty))
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
            for branch in &parallel.branches {
                collect_block_context(&branch.body, cursor, providers, context);
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
    context
        .bindings
        .insert(id.to_string(), statement_output_type(stmt, providers));
}

/// the output type a straight-line successor would see as `prev`: the node's explicit `label_type`,
/// else its provider action / subflow result shape, else `Any` for control-flow and effect-free
/// nodes.
fn statement_output_type(stmt: &Stmt, providers: &[ProviderMetadata]) -> RuninatorType {
    if let Some(label_type) = &stmt.label_type {
        return lower_type(label_type).unwrap_or(RuninatorType::Any);
    }
    match &stmt.kind {
        StmtKind::Action(action) => {
            provider_action_output_type(providers, &action.provider, &action.function)
                .unwrap_or(RuninatorType::Any)
        }
        StmtKind::Subflow(subflow) => subflow_output_type(subflow.detached),
        StmtKind::For(for_stmt) => for_stmt
            .body
            .last()
            .map(|last| RuninatorType::array(statement_output_type(last, providers)))
            .unwrap_or_else(|| RuninatorType::array(RuninatorType::Any)),
        _ => RuninatorType::Any,
    }
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
            let els_ty = infer_expr_type(els, context)?;
            Some(then_ty.unify(&els_ty))
        }
        ExprKind::InlineCode { .. } => Some(RuninatorType::String),
        // a cast reports its asserted target type, so completion off the cast offers that shape.
        ExprKind::Cast { ty, .. } => lower_type(ty).ok(),
        // applying a callee yields the callee function's result type, so completion can descend into it.
        ExprKind::Apply { callee, .. } => match infer_expr_type(callee, context)? {
            RuninatorType::Function { ret, .. } => Some(*ret),
            _ => Some(RuninatorType::Any),
        },
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
        ExprKind::Call { name, args, .. } => {
            if runinator_compute::is_higher_order(name) {
                // recover the higher-order result from the collection + lambda body, falling back to
                // `any` when the collection type or lambda shape is not statically determinable.
                infer_higher_order_type(name, args, context).or(Some(RuninatorType::Any))
            } else if let Some(RuninatorType::Function { ret, .. }) = function_local(name, context)
            {
                // a call to a local bound to a first-class lambda yields the function's result type.
                Some((**ret).clone())
            } else {
                // mirror sema: recover an argument-dependent result for the polymorphic
                // intrinsics, else the intrinsic's first declared result type, else `Any`.
                let arg_types = args
                    .iter()
                    .map(|arg| infer_expr_type(arg, context).unwrap_or(RuninatorType::Any))
                    .collect::<Vec<_>>();
                let literal_keys = args
                    .get(1)
                    .and_then(runinator_rexrap::ast::static_string_keys);
                Some(
                    runinator_compute::intrinsic_result_type(
                        name,
                        &arg_types,
                        literal_keys.as_deref(),
                    )
                    .or_else(|| {
                        runinator_compute::intrinsic_signature(name)
                            .and_then(|sig| sig.results.first().map(|result| result.ty.clone()))
                    })
                    .unwrap_or(RuninatorType::Any),
                )
            }
        }
        // a lambda's value type: unconstrained parameters and the body's result type.
        ExprKind::Lambda { params, body } => {
            let mut scoped = context.clone();
            for param in params {
                scoped.scoped.insert(param.clone(), RuninatorType::Any);
            }
            let ret = infer_expr_type(body, &scoped).unwrap_or(RuninatorType::Any);
            Some(RuninatorType::Function {
                params: params.iter().map(|_| RuninatorType::Any).collect(),
                ret: Box::new(ret),
            })
        }
        ExprKind::Path(segs) => infer_path_type(segs, context),
        // a spread carries no value type of its own; it is resolved by desugaring.
        ExprKind::Spread(_) => None,
    }
}

/// the result type of a higher-order intrinsic call, derived from the collection element type and
/// the lambda body. mirrors sema's inference so autocomplete after a `map`/`filter`/... sees the
/// same concrete type. `None` when the collection or lambda shape is not statically determinable.
fn infer_higher_order_type(
    name: &str,
    args: &[Expr],
    context: &CompletionContext,
) -> Option<RuninatorType> {
    let collection_type = infer_expr_type(args.first()?, context)?;
    let item_type = higher_order_item_type(&collection_type)?;
    match name {
        "map" => Some(RuninatorType::array(infer_lambda_type(
            args.get(1)?,
            &[(0, item_type)],
            context,
        )?)),
        "flat_map" => match infer_lambda_type(args.get(1)?, &[(0, item_type)], context)? {
            RuninatorType::Array(inner) => Some(RuninatorType::array(*inner)),
            other => Some(RuninatorType::array(other)),
        },
        // filter/sort_by keep the element type; the lambda only refines ordering/selection.
        "filter" | "sort_by" => Some(RuninatorType::array(item_type)),
        "find" => Some(RuninatorType::Union(vec![item_type, RuninatorType::Null])),
        "any" | "all" => Some(RuninatorType::Boolean),
        "reduce" => {
            let accumulator = infer_expr_type(args.get(1)?, context)?;
            let body = infer_lambda_type(
                args.get(2)?,
                &[(0, accumulator.clone()), (1, item_type)],
                context,
            )?;
            Some(accumulator.common_type(&body).unwrap_or(accumulator))
        }
        _ => None,
    }
}

/// the element type of a higher-order collection argument (`array<T>` -> `T`, union of arrays ->
/// union of elements, `any` -> `any`); `None` for a non-iterable.
fn higher_order_item_type(ty: &RuninatorType) -> Option<RuninatorType> {
    match ty {
        RuninatorType::Union(_) => ty.union_element_type(),
        other => other.element_type(),
    }
}

/// find a local bound to a first-class function type (a lambda value), if any.
fn function_local<'a>(name: &str, context: &'a CompletionContext) -> Option<&'a RuninatorType> {
    context
        .scoped
        .get(name)
        .or_else(|| context.bindings.get(name))
        .filter(|ty| matches!(ty, RuninatorType::Function { .. }))
}

/// infer a lambda body's type with its parameters bound to the given element types in a scoped copy
/// of the completion context, or use a first-class function argument's declared result type. `None`
/// when the expression is neither a lambda nor a function of the expected arity.
fn infer_lambda_type(
    expr: &Expr,
    bindings: &[(usize, RuninatorType)],
    context: &CompletionContext,
) -> Option<RuninatorType> {
    let ExprKind::Lambda { params, body } = &expr.kind else {
        return match infer_expr_type(expr, context) {
            Some(RuninatorType::Function { params, ret }) if params.len() == bindings.len() => {
                Some(*ret)
            }
            _ => None,
        };
    };
    if params.len() != bindings.len() {
        return None;
    }
    let mut scoped = context.clone();
    for (index, ty) in bindings {
        scoped.scoped.insert(params[*index].clone(), ty.clone());
    }
    infer_expr_type(body, &scoped)
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

pub(crate) fn workflow_context_type() -> RuninatorType {
    runinator_models::workflow_state::WorkflowContextHeader::runinator_type()
}

fn subflow_output_type(detached: bool) -> RuninatorType {
    // a detached subflow is fire-and-forget, so its output snapshot is never populated: `state` is
    // `Null` (no fields to complete) rather than `Any`, matching the author-time type checker.
    let state = if detached {
        RuninatorType::Null
    } else {
        RuninatorType::Any
    };
    RuninatorType::structure([
        ("subflow_run_id", RuninatorType::Integer),
        ("subflow_workflow_id", RuninatorType::Integer),
        ("run_name", RuninatorType::String),
        ("reused", RuninatorType::Boolean),
        ("status", RuninatorType::String),
        ("state", state),
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

#[derive(Debug, Clone)]
pub(crate) struct PathContext {
    pub(crate) head: String,
    pub(crate) completed: Vec<String>,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
}
