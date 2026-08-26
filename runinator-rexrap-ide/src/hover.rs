use runinator_models::{
    providers::{ActionMetadata, ProviderMetadata, RuninatorType},
    settings::{SettingKind, SettingSummary},
    types::RuninatorField,
};
use serde::{Deserialize, Serialize};

use runinator_rexrap::{
    analysis::{GRAMMAR_KEYWORDS, lower_type_with, resolve_named_types},
    ast::{Document, FnBody, FunctionDef},
    parse_document,
};

use crate::completion::{
    CompletionContext, action_signature, completion_context, find_provider, navigate_type,
    root_type, type_fields, workflow_context_type,
};
use crate::cursor::{Cursor, clamp_to_char_boundary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RexRapHoverRequest {
    pub source: String,
    pub cursor_byte: usize,
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
    #[serde(default)]
    pub settings: Vec<SettingSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RexRapHoverResponse {
    pub range_start_byte: usize,
    pub range_end_byte: usize,
    pub title: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

/// resolve editor hover information at a byte cursor using provider metadata and local type context.
pub fn hover_source(request: RexRapHoverRequest) -> Option<RexRapHoverResponse> {
    let source = request.source;
    let pos = clamp_to_char_boundary(&source, request.cursor_byte);
    let cursor = Cursor::new(&source, pos);
    let document = parse_document(&source).ok()?;
    let context = completion_context(&source, pos, &request.providers);
    let word = cursor.word_at()?;

    action_argument_hover(cursor, word, &request.providers)
        .or_else(|| action_hover(cursor, word, &request.providers))
        .or_else(|| path_hover(cursor, word, &context, &request.settings))
        .or_else(|| type_hover(&document, word))
        .or_else(|| function_hover(&document, word))
        .or_else(|| bare_symbol_hover(word, &context, &request.providers))
        .or_else(|| keyword_hover(word))
}

fn action_argument_hover(
    cursor: Cursor<'_>,
    word: WordAt<'_>,
    providers: &[ProviderMetadata],
) -> Option<RexRapHoverResponse> {
    let colon = cursor.at(word.end).next_non_space()?;
    if cursor.source.as_bytes().get(colon) != Some(&b':') {
        return None;
    }
    let open = cursor.unmatched_open_paren()?;
    if open > word.start {
        return None;
    }
    let (provider_name, action_name) = call_name_before(cursor.source, open)?;
    let action = find_provider(providers, provider_name)?
        .actions
        .iter()
        .find(|action| action.function_name == action_name)?;
    let parameter = action
        .parameters
        .iter()
        .find(|parameter| parameter.name == word.text)?;
    let required = if parameter.required {
        "required"
    } else {
        "optional"
    };
    Some(RexRapHoverResponse {
        range_start_byte: word.start,
        range_end_byte: word.end,
        title: parameter.name.clone(),
        kind: "parameter".into(),
        detail: Some(format!("{required} {}", render_type(&parameter.ty))),
        documentation: parameter
            .description
            .clone()
            .or_else(|| parameter.label.clone()),
    })
}

fn action_hover(
    cursor: Cursor<'_>,
    word: WordAt<'_>,
    providers: &[ProviderMetadata],
) -> Option<RexRapHoverResponse> {
    let token = cursor.action_token_at()?;
    // the *last* dot, not the first: the grammar reads every segment but the last as the provider,
    // so `functions.image_tools.resize` is provider `functions.image_tools` and action `resize`.
    // splitting on the first dot would report the provider as `functions` and the action as
    // `image_tools.resize`, and find neither.
    let dot = token.text.rfind('.')?;
    let provider_name = &token.text[..dot];
    let action_name = &token.text[dot + 1..];
    if word.start < token.start + dot {
        let provider = find_provider(providers, provider_name)?;
        return Some(RexRapHoverResponse {
            range_start_byte: token.start,
            range_end_byte: token.start + dot,
            title: provider.name.clone(),
            kind: "provider".into(),
            detail: Some(format!(
                "{} action{}",
                provider.actions.len(),
                plural(provider.actions.len())
            )),
            documentation: provider.metadata.contract.clone(),
        });
    }
    let provider = find_provider(providers, provider_name)?;
    let action = provider
        .actions
        .iter()
        .find(|action| action.function_name == action_name)?;
    Some(action_response(
        provider_name,
        action,
        token.start + dot + 1,
        token.end,
    ))
}

fn path_hover(
    cursor: Cursor<'_>,
    word: WordAt<'_>,
    context: &CompletionContext,
    settings: &[SettingSummary],
) -> Option<RexRapHoverResponse> {
    let path = cursor.path_at()?;
    if path.parts.len() <= 1 {
        return None;
    }
    let index = path.segment_index_at(word.start)?;
    if path.parts[0] == "std" {
        return std_path_hover(path, index);
    }
    if path.parts[0] == "config" || path.parts[0] == "secret" {
        return setting_path_hover(path, index, settings);
    }
    if let Some(module) = context.namespace.aliases.get(path.parts[0]) {
        return alias_path_hover(path, index, module);
    }
    typed_path_hover(path, index, context)
}

fn std_path_hover(path: HoverPath<'_>, index: usize) -> Option<RexRapHoverResponse> {
    match (index, path.parts.as_slice()) {
        (0, _) => Some(RexRapHoverResponse {
            range_start_byte: path.ranges[0].0,
            range_end_byte: path.ranges[0].1,
            title: "std".into(),
            kind: "namespace".into(),
            detail: Some("standard library".into()),
            documentation: Some("Built-in compute functions grouped by module.".into()),
        }),
        (1, [_, module, ..]) => Some(RexRapHoverResponse {
            range_start_byte: path.ranges[1].0,
            range_end_byte: path.ranges[1].1,
            title: (*module).into(),
            kind: "module".into(),
            detail: Some(format!("std.{module}")),
            documentation: None,
        }),
        (2, [_, module, leaf]) if runinator_compute::intrinsic_module(leaf) == Some(*module) => {
            intrinsic_response(leaf, path.ranges[2])
        }
        _ => None,
    }
}

fn alias_path_hover(
    path: HoverPath<'_>,
    index: usize,
    module: &str,
) -> Option<RexRapHoverResponse> {
    if index == 0 {
        return Some(RexRapHoverResponse {
            range_start_byte: path.ranges[0].0,
            range_end_byte: path.ranges[0].1,
            title: path.parts[0].into(),
            kind: "module alias".into(),
            detail: Some(format!("std.{module}")),
            documentation: None,
        });
    }
    if index == 1 && path.parts.len() == 2 {
        let leaf = path.parts[1];
        if runinator_compute::intrinsic_module(leaf) == Some(module) {
            return intrinsic_response(leaf, path.ranges[1]);
        }
    }
    None
}

fn setting_path_hover(
    path: HoverPath<'_>,
    index: usize,
    settings: &[SettingSummary],
) -> Option<RexRapHoverResponse> {
    let kind = if path.parts[0] == "secret" {
        SettingKind::Secret
    } else {
        SettingKind::Config
    };
    match (index, path.parts.as_slice()) {
        (0, _) => Some(RexRapHoverResponse {
            range_start_byte: path.ranges[0].0,
            range_end_byte: path.ranges[0].1,
            title: path.parts[0].into(),
            kind: "setting root".into(),
            detail: Some(kind.as_str().into()),
            documentation: Some(format!(
                "References {} values available at run time.",
                kind.as_str()
            )),
        }),
        (1, [_, scope, ..]) => Some(RexRapHoverResponse {
            range_start_byte: path.ranges[1].0,
            range_end_byte: path.ranges[1].1,
            title: (*scope).into(),
            kind: "setting scope".into(),
            detail: Some(kind.as_str().into()),
            documentation: None,
        }),
        (2, [_, scope, name]) => {
            let setting = settings.iter().find(|setting| {
                setting.kind == kind && setting.scope == *scope && setting.name == *name
            })?;
            Some(RexRapHoverResponse {
                range_start_byte: path.ranges[2].0,
                range_end_byte: path.ranges[2].1,
                title: setting.name.clone(),
                kind: "setting".into(),
                detail: Some(format!(
                    "{}.{}.{}",
                    kind.as_str(),
                    setting.scope,
                    setting.name
                )),
                documentation: None,
            })
        }
        _ => None,
    }
}

fn typed_path_hover(
    path: HoverPath<'_>,
    index: usize,
    context: &CompletionContext,
) -> Option<RexRapHoverResponse> {
    if index == 0 {
        let ty = root_type(path.parts[0], context)?;
        return Some(RexRapHoverResponse {
            range_start_byte: path.ranges[0].0,
            range_end_byte: path.ranges[0].1,
            title: path.parts[0].into(),
            kind: path_root_kind(path.parts[0], context).into(),
            detail: Some(render_type(&ty)),
            documentation: path_root_docs(path.parts[0]),
        });
    }
    let root = root_type(path.parts[0], context)?;
    let parent_path = path.parts[1..index]
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    let parent = navigate_type(root, &parent_path)?;
    let current = path.parts[index];
    let field = type_fields(&parent)?
        .into_iter()
        .find(|(name, _)| name == current)
        .map(|(_, field)| field)?;
    Some(field_response(current, &field, path.ranges[index]))
}

fn type_hover(document: &Document, word: WordAt<'_>) -> Option<RexRapHoverResponse> {
    let type_decls = document
        .workflows
        .iter()
        .flat_map(|workflow| workflow.type_decls.clone())
        .collect::<Vec<_>>();
    let named = resolve_named_types(&type_decls).ok()?;
    if let Some(ty) = named.get(word.text) {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: word.text.into(),
            kind: "type".into(),
            detail: Some(render_type(ty)),
            documentation: None,
        });
    }
    primitive_type(word.text).map(|ty| RexRapHoverResponse {
        range_start_byte: word.start,
        range_end_byte: word.end,
        title: word.text.into(),
        kind: "type".into(),
        detail: Some(render_type(&ty)),
        documentation: Some("Built-in REXRAP type.".into()),
    })
}

fn function_hover(document: &Document, word: WordAt<'_>) -> Option<RexRapHoverResponse> {
    let function = document
        .functions
        .iter()
        .find(|function| function.name == word.text)?;
    Some(RexRapHoverResponse {
        range_start_byte: word.start,
        range_end_byte: word.end,
        title: function.name.clone(),
        kind: "function".into(),
        detail: Some(function_signature(function)),
        documentation: function_docs(function),
    })
}

fn bare_symbol_hover(
    word: WordAt<'_>,
    context: &CompletionContext,
    providers: &[ProviderMetadata],
) -> Option<RexRapHoverResponse> {
    if word.text == "params" {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: "params".into(),
            kind: "parameter root".into(),
            detail: Some(render_type(&context.input)),
            documentation: Some("Workflow input parameters.".into()),
        });
    }
    if word.text == "run" {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: "run".into(),
            kind: "run context".into(),
            detail: Some(render_type(&workflow_context_type())),
            documentation: Some("Current workflow run metadata.".into()),
        });
    }
    if let Some(provider) = find_provider(providers, word.text) {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: provider.name.clone(),
            kind: "provider".into(),
            detail: Some(format!(
                "{} action{}",
                provider.actions.len(),
                plural(provider.actions.len())
            )),
            documentation: provider.metadata.contract.clone(),
        });
    }
    if let Some(ty) = context
        .scoped
        .get(word.text)
        .or_else(|| context.bindings.get(word.text))
    {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: word.text.into(),
            kind: path_root_kind(word.text, context).into(),
            detail: Some(render_type(ty)),
            documentation: None,
        });
    }
    if collect_labels_from_context(context).contains(word.text) {
        return Some(RexRapHoverResponse {
            range_start_byte: word.start,
            range_end_byte: word.end,
            title: word.text.into(),
            kind: "node".into(),
            detail: Some("workflow node".into()),
            documentation: None,
        });
    }
    if context.namespace.bare_intrinsics.contains(word.text) {
        return intrinsic_response(word.text, (word.start, word.end));
    }
    None
}

fn keyword_hover(word: WordAt<'_>) -> Option<RexRapHoverResponse> {
    let docs = match word.text {
        "workflow" => "Declares a workflow and its body.",
        "params" => "Declares workflow input parameters.",
        "type" => "Declares a reusable named type.",
        "let" => {
            "Binds a step's result. A plain call joins inline; an `async` call yields a \
                  `task[T]` to `await`."
        }
        "async" => {
            "Schedules this call as a task instead of joining it inline. Asyncness is a \
                    property of the call site, never of the callee."
        }
        "await" => "Joins a `task[T]` handle and yields its result.",
        "detach" => "Drops a task handle without joining it.",
        "do" => "The runtime block: the statements a run executes.",
        "routes" => "The statement's outgoing edges, each arm handing control on with `continue`.",
        "join" => "A named continuation, entered only by an explicit `continue <name>`.",
        "compute" => "A pure computation block, folded by the compute VM.",
        "if" => "Runs a branch when its condition is true.",
        "for" | "map" => "Iterates over a collection.",
        "while" | "until" => "Repeats a body while the condition holds.",
        "match" => "Selects a branch by equality or predicate.",
        "parallel" => "Runs branches concurrently and joins them.",
        "race" => "Runs branches concurrently and continues with a winner policy.",
        "try" => "Runs a body with optional catch and finally branches.",
        "subflow" => "Runs a workflow as a subflow.",
        "wait" => "Parks the workflow until a duration or state is ready.",
        "approve" => "Parks the workflow for human approval.",
        "gate" => "Parks the workflow behind an external or condition gate.",
        "on_timeout" => "Selects whether an expired gate fails or continues.",
        "signal" => "Waits for an external signal.",
        "interrupt" => {
            "Declares a handler region that runs when the named source fires, suspending the run's \
             thread of control until a `resume` inside it hands control back. Sources: `wake` (a \
             wait deadline elapsed), `timeout` (the node's deadline is about to blow), `retry` \
             (before a re-dispatch), `failure` (a node run settled failed), `resolved` (a signal, \
             approval or input landed), `child` (a subflow child reached a terminal), `external` \
             (POST /workflow_runs/{id}/interrupts), `orphan_signal` (a signal nothing was waiting \
             for)."
        }
        "resume" => {
            "Ends an interrupt handler and returns control to the interrupted thread: bare resumes \
             at that node, `next` takes its success edge, `restart` re-enters it, `fail` takes its \
             on_failure edge."
        }
        "emit" => "Emits workflow output data (shorthand for an output node with no artifacts).",
        "output" => "Declares run-level artifacts and/or emits an event from an output block.",
        "yield" => "Returns a value from a control region.",
        "trigger" => "Declares an import-managed workflow trigger.",
        "on_success" | "on_failure" | "on_complete" => {
            "Chains another workflow to start when this run reaches that terminal state."
        }
        "watch" => "Declares a workflow-level cancellation guard.",
        "fn" => "Declares a reusable compute function.",
        "import" => "Imports a namespace or standard-library module.",
        "alias" => "Declares a reusable argument object.",
        _ if GRAMMAR_KEYWORDS.binary_search(&word.text).is_ok() => "REXRAP keyword.",
        _ => return None,
    };
    Some(RexRapHoverResponse {
        range_start_byte: word.start,
        range_end_byte: word.end,
        title: word.text.into(),
        kind: "keyword".into(),
        detail: None,
        documentation: Some(docs.into()),
    })
}

fn action_response(
    provider_name: &str,
    action: &ActionMetadata,
    start: usize,
    end: usize,
) -> RexRapHoverResponse {
    let output = action.results_type();
    RexRapHoverResponse {
        range_start_byte: start,
        range_end_byte: end,
        title: format!("{provider_name}.{}", action.function_name),
        kind: "action".into(),
        detail: Some(format!(
            "{} -> {}",
            action_signature(action),
            render_type(&output)
        )),
        documentation: action.description.clone(),
    }
}

fn intrinsic_response(name: &str, range: (usize, usize)) -> Option<RexRapHoverResponse> {
    let action = runinator_compute::intrinsic_signature(name)?;
    Some(RexRapHoverResponse {
        range_start_byte: range.0,
        range_end_byte: range.1,
        title: name.into(),
        kind: "function".into(),
        detail: Some(format!(
            "{} -> {}",
            action_signature(&action),
            render_type(&action.results_type())
        )),
        documentation: action.description,
    })
}

fn field_response(
    name: &str,
    field: &RuninatorField,
    range: (usize, usize),
) -> RexRapHoverResponse {
    let required = if field.required {
        "required"
    } else {
        "optional"
    };
    RexRapHoverResponse {
        range_start_byte: range.0,
        range_end_byte: range.1,
        title: name.into(),
        kind: "field".into(),
        detail: Some(format!("{required} {}", render_type(&field.ty))),
        documentation: None,
    }
}

fn path_root_kind(name: &str, context: &CompletionContext) -> &'static str {
    if name == "params" {
        "parameter root"
    } else if name == "run" {
        "run context"
    } else if context.scoped.contains_key(name) {
        "local"
    } else if context.bindings.contains_key(name) {
        "node"
    } else {
        "value"
    }
}

fn path_root_docs(name: &str) -> Option<String> {
    match name {
        "params" => Some("Workflow input parameters.".into()),
        "run" => Some("Current workflow run metadata.".into()),
        _ => None,
    }
}

fn primitive_type(name: &str) -> Option<RuninatorType> {
    match name {
        "string" => Some(RuninatorType::String),
        "integer" | "int" => Some(RuninatorType::Integer),
        "number" | "float" => Some(RuninatorType::Number),
        "duration" => Some(RuninatorType::Duration),
        "file" => Some(RuninatorType::File),
        "boolean" | "bool" => Some(RuninatorType::Boolean),
        "null" => Some(RuninatorType::Null),
        "any" | "json" => Some(RuninatorType::Any),
        _ => None,
    }
}

fn render_type(ty: &RuninatorType) -> String {
    render_type_with_depth(ty, 0)
}

fn render_type_with_depth(ty: &RuninatorType, depth: usize) -> String {
    if depth > 2 {
        return ty.describe().into();
    }
    match ty {
        RuninatorType::Null
        | RuninatorType::Boolean
        | RuninatorType::Integer
        | RuninatorType::Number
        | RuninatorType::Duration
        | RuninatorType::String
        | RuninatorType::File
        | RuninatorType::Any => ty.describe().into(),
        RuninatorType::Enum(values) => format!(
            "enum[{}]",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        RuninatorType::Range { base, min, max } => {
            let min = min
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_default();
            let max = max
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_default();
            format!(
                "{} range {min}..{max}",
                render_type_with_depth(base, depth + 1)
            )
        }
        RuninatorType::Array(items) => format!("{}[]", render_type_with_depth(items, depth + 1)),
        RuninatorType::Map(values) => format!("map<{}>", render_type_with_depth(values, depth + 1)),
        RuninatorType::Struct { fields, additional } => {
            let mut rendered = fields
                .iter()
                .take(6)
                .map(|(name, field)| {
                    let suffix = if field.required { "" } else { "?" };
                    format!(
                        "{name}{suffix}: {}",
                        render_type_with_depth(&field.ty, depth + 1)
                    )
                })
                .collect::<Vec<_>>();
            if fields.len() > rendered.len() {
                rendered.push("...".into());
            }
            if let Some(additional) = additional {
                rendered.push(format!(
                    "...: {}",
                    render_type_with_depth(additional, depth + 1)
                ));
            }
            format!("{{ {} }}", rendered.join(", "))
        }
        RuninatorType::Union(variants) => variants
            .iter()
            .map(|variant| render_type_with_depth(variant, depth + 1))
            .collect::<Vec<_>>()
            .join(" | "),
        RuninatorType::Function { params, ret } => format!(
            "function({}) -> {}",
            params
                .iter()
                .map(|param| render_type_with_depth(param, depth + 1))
                .collect::<Vec<_>>()
                .join(", "),
            render_type_with_depth(ret, depth + 1)
        ),
    }
}

fn function_signature(function: &FunctionDef) -> String {
    let named = resolve_named_types(&[]).unwrap_or_default();
    let params = function
        .params
        .iter()
        .map(|param| {
            let suffix = if param.optional || param.default.is_some() {
                "?"
            } else {
                ""
            };
            let ty = lower_type_with(&param.ty, &named)
                .map(|ty| render_type(&ty))
                .unwrap_or_else(|_| "any".into());
            format!("{}{suffix}: {ty}", param.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = function
        .ret
        .as_ref()
        .and_then(|ty| lower_type_with(ty, &named).ok())
        .map(|ty| render_type(&ty))
        .unwrap_or_else(|| "any".into());
    format!("({params}) -> {ret}")
}

fn function_docs(function: &FunctionDef) -> Option<String> {
    match &function.body {
        FnBody::Expr(_) => Some("User-defined expression function.".into()),
        FnBody::Block(_) => Some("User-defined compute function.".into()),
        FnBody::Run(_) => Some(
            "User-defined runtime function (`task fn`). Call it directly to join it inline, or \
             `async` it for a `task[T]` handle."
                .into(),
        ),
    }
}

fn collect_labels_from_context(context: &CompletionContext) -> &std::collections::BTreeSet<String> {
    &context.labels
}

fn call_name_before(source: &str, open: usize) -> Option<(&str, &str)> {
    let before_open = source[..open].trim_end();
    let dot = before_open.rfind('.')?;
    let action_start = identifier_start_before(before_open, before_open.len(), true)?;
    if action_start <= dot {
        return None;
    }
    let provider_start = identifier_start_before(before_open, dot, true)?;
    Some((
        &before_open[provider_start..dot],
        &before_open[action_start..],
    ))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WordAt<'a> {
    pub(crate) text: &'a str,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug)]
pub(crate) struct HoverPath<'a> {
    pub(crate) parts: Vec<&'a str>,
    pub(crate) ranges: Vec<(usize, usize)>,
}

impl HoverPath<'_> {
    fn segment_index_at(&self, offset: usize) -> Option<usize> {
        self.ranges
            .iter()
            .position(|(start, end)| *start <= offset && offset <= *end)
    }
}

fn identifier_start_before(source: &str, end: usize, allow_hyphen: bool) -> Option<usize> {
    if end == 0 {
        return None;
    }
    let bytes = source.as_bytes();
    let mut start = end;
    while start > 0 {
        let byte = bytes[start - 1];
        if !(byte.is_ascii_alphanumeric() || byte == b'_' || (allow_hyphen && byte == b'-')) {
            break;
        }
        start -= 1;
    }
    (start < end).then_some(start)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
