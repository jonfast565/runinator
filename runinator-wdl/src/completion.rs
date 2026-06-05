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

#[derive(Debug, Clone)]
struct CompletionContext {
    input: RuninatorType,
    bindings: BTreeMap<String, RuninatorType>,
    scoped: BTreeMap<String, RuninatorType>,
}

#[derive(Debug, Clone)]
struct ActionCallContext {
    provider: String,
    action: String,
    replace_start: usize,
    replace_end: usize,
    used_args: BTreeSet<String>,
}

/// complete wdl at a byte cursor using provider metadata and local type context.
pub fn complete_source(request: WdlCompletionRequest) -> WdlCompletionResponse {
    let source = request.source;
    let cursor = clamp_to_char_boundary(&source, request.cursor_byte);
    let word = current_word(&source, cursor);

    if !is_completion_allowed(&source, cursor) {
        return empty_response(word.start, cursor);
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
    if let Some(path) = path_context(&source, cursor) {
        if path.head == "config" || path.head == "secret" {
            return complete_setting_path(&request.settings, path);
        }
        if let Some(response) = complete_path(path, &context) {
            return response;
        }
    }

    if let Some(call) = action_call_context(&source, cursor) {
        return complete_action_args(&request.providers, call);
    }

    complete_providers(&request.providers, word.start, cursor)
}

fn empty_response(replace_start_byte: usize, replace_end_byte: usize) -> WdlCompletionResponse {
    WdlCompletionResponse {
        replace_start_byte,
        replace_end_byte,
        items: Vec::new(),
    }
}

fn complete_providers(
    providers: &[ProviderMetadata],
    replace_start: usize,
    replace_end: usize,
) -> WdlCompletionResponse {
    let mut items = providers
        .iter()
        .map(|provider| WdlCompletionItem {
            label: provider.name.clone(),
            kind: "class".into(),
            detail: Some("provider".into()),
            documentation: None,
            insert_text: provider.name.clone(),
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
    let items = labels
        .into_iter()
        .map(|label| WdlCompletionItem {
            label: label.clone(),
            kind: "variable".into(),
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
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).ok()?
        }
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

fn root_type(name: &str, context: &CompletionContext) -> Option<RuninatorType> {
    if name == "input" {
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

fn type_fields(ty: &RuninatorType) -> Option<Vec<(String, RuninatorField)>> {
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

fn navigate_type(mut ty: RuninatorType, segs: &[String]) -> Option<RuninatorType> {
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

fn completion_context(
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
        return CompletionContext {
            input: RuninatorType::Any,
            bindings: BTreeMap::new(),
            scoped: BTreeMap::new(),
        };
    };
    let input = document
        .workflow
        .input
        .as_ref()
        .and_then(|ty| lower_type(ty).ok())
        .unwrap_or(RuninatorType::Any);
    let mut context = CompletionContext {
        input,
        bindings: BTreeMap::new(),
        scoped: BTreeMap::new(),
    };
    collect_block_context(&document.workflow.body, cursor, providers, &mut context);
    context
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

fn provider_action_output_type(
    providers: &[ProviderMetadata],
    provider_name: &str,
    action_name: &str,
) -> Option<RuninatorType> {
    let action = find_provider(providers, provider_name)?
        .actions
        .iter()
        .find(|action| action.function_name == action_name)?;
    Some(RuninatorType::structure(
        action
            .results
            .iter()
            .map(|result| (result.name.clone(), result.ty.clone())),
    ))
}

fn infer_expr_type(expr: &Expr, context: &CompletionContext) -> Option<RuninatorType> {
    match &expr.kind {
        ExprKind::Null => Some(RuninatorType::Null),
        ExprKind::Bool(_) => Some(RuninatorType::Boolean),
        ExprKind::Int(_) => Some(RuninatorType::Integer),
        ExprKind::Float(_) => Some(RuninatorType::Number),
        ExprKind::Str(_) => Some(RuninatorType::String),
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

fn workflow_context_type() -> RuninatorType {
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

fn find_provider<'a>(
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

fn action_signature(action: &ActionMetadata) -> String {
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
struct PathContext {
    head: String,
    completed: Vec<String>,
    replace_start: usize,
    replace_end: usize,
}

fn path_context(source: &str, cursor: usize) -> Option<PathContext> {
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

fn unmatched_open_paren(source: &str, cursor: usize) -> Option<usize> {
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

fn clamp_to_char_boundary(source: &str, cursor: usize) -> usize {
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
