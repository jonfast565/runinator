// runinator-rexrap: a human-friendly workflow language that transpiles to the existing
// runinator json workflow model. parse rexrap text -> ast -> WorkflowDefinition, and
// decompile a WorkflowDefinition back to rexrap text. the runtime is unchanged; this crate
// is purely an author-time front end.
//
// the language core is split by compile stage — `runinator-rexrap-syntax` (text <-> ast),
// `runinator-rexrap-sema` (ast -> diagnostics), `runinator-rexrap-codegen` (ast <-> json model) —
// This crate assembles them. It owns the public API every consumer links against, the
// `.rexrapp`/`.rexraps` front ends, the `analysis` seam published for the editor crate, and the
// test suite, which is the first place parse, lower, decompile, and format are all visible at
// once (round-trip and format-idempotence are exactly those cross-stage contracts).

use runinator_models::providers::ProviderMetadata;
use runinator_models::types::{RuninatorField, RuninatorType};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowDefinition;
use runinator_rexrap_syntax::format;
use serde::{Deserialize, Serialize};

pub mod analysis;
mod pipeline;
mod rrx;
mod secrets;

// the language core, re-exported at its historical paths so consumers name one crate.
pub use runinator_rexrap_codegen::{DecompileOptions, lower::NodeSpan};
pub use runinator_rexrap_sema::sema;
pub use runinator_rexrap_sema::{CompileOptions, TypePolicy, WorkflowSignature};
pub use runinator_rexrap_syntax::{ast, comments, errors};

pub use errors::{RexRapError, Span};
pub use pipeline::{parse_pipeline_str, pipeline_to_rexrapp};
pub use rrx::{RrxBlocks, parse_rrx_blocks};
pub use runinator_rexrap_syntax::included_file_paths;
pub use runinator_rexrap_syntax::{
    ConsoleModule, parse_condition_fragment, parse_console_module, parse_do_fragment,
    parse_document, parse_expression_fragment,
};
pub use secrets::{parse_secrets_str, parse_settings_str, secrets_to_rexraps, settings_to_rexrap};
pub use sema::{Diagnostic, Severity};

use runinator_rexrap_codegen::{decompile, lower};
use runinator_rexrap_sema::{desugar, namespace};

/// the supported standalone REXRAP fragment surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RexRapFragmentKind {
    Expression,
    Condition,
    Do,
}

/// One top-level REXRAP declaration, retained as standalone source so a console session can carry
/// it into a later cell without inventing a second function representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RexRapFunctionDefinition {
    pub name: String,
    pub is_task: bool,
    pub source: String,
}

/// Read the top-level function declarations from either a complete document or a console module.
/// The snippets deliberately exclude neighbouring workflow text and are therefore safe to prepend
/// to a later scratch document.
pub fn function_definitions(src: &str) -> Result<Vec<RexRapFunctionDefinition>, RexRapError> {
    let functions = match parse_document(src) {
        Ok(document) => document.functions,
        Err(_) => parse_console_module(src)?.functions,
    };
    functions
        .into_iter()
        .map(|function| {
            let source = src
                .get(function.span.start..function.span.end)
                .ok_or_else(|| RexRapError::lower("function span falls outside its source"))?
                .to_string();
            Ok(RexRapFunctionDefinition {
                name: function.name,
                is_task: function.is_task,
                source,
            })
        })
        .collect()
}

/// compile rexrap source into a validated WorkflowDefinition. semantic errors block the
/// compile; warnings are dropped (use `compile_str_with_diagnostics` to inspect them).
pub fn compile_str(src: &str, options: &CompileOptions) -> Result<WorkflowDefinition, RexRapError> {
    compile_str_with_diagnostics(src, options).map(|(definition, _)| definition)
}

pub fn workflow_signature_from_source(src: &str) -> Result<Vec<WorkflowSignature>, RexRapError> {
    let document = parse_document(src)?;
    let mut signatures = Vec::new();
    for workflow in &document.workflows {
        let named = runinator_rexrap_sema::types::resolve_named_types(&workflow.type_decls)?;
        let input = match &workflow.input {
            Some(input) => lower_signature_input_type(input, &named)?,
            None => RuninatorType::Any,
        };
        let output = match &workflow.output {
            Some(output) => runinator_rexrap_sema::types::lower_type_with(output, &named)?,
            None => RuninatorType::Any,
        };
        signatures.push(WorkflowSignature {
            name: workflow.name.clone(),
            input: input.clone(),
            output: output.clone(),
        });
        if let Some(namespace) = &workflow.namespace {
            signatures.push(WorkflowSignature {
                name: format!("{namespace}.{}", workflow.name),
                input,
                output,
            });
        }
    }
    Ok(signatures)
}

fn lower_signature_input_type(
    type_expr: &ast::TypeExpr,
    named: &runinator_rexrap_sema::types::NamedTypes,
) -> Result<RuninatorType, RexRapError> {
    let ast::TypeExpr::Struct { fields, additional } = type_expr else {
        return runinator_rexrap_sema::types::lower_type_with(type_expr, named);
    };
    let mut mapped = std::collections::BTreeMap::new();
    for field in fields {
        let ty = runinator_rexrap_sema::types::lower_type_with(&field.ty, named)?;
        let runinator_field = if field.optional || field.default.is_some() {
            RuninatorField::optional(ty)
        } else {
            RuninatorField::required(ty)
        };
        mapped.insert(field.name.clone(), runinator_field);
    }
    let additional = additional
        .as_ref()
        .map(|ty| runinator_rexrap_sema::types::lower_type_with(ty, named))
        .transpose()?
        .map(Box::new);
    Ok(RuninatorType::Struct {
        fields: mapped,
        additional,
    })
}

/// like `compile_str`, but also returns the advisory (warning) diagnostics. semantic errors
/// still short-circuit with `RexRapError::Semantic`.
pub fn compile_str_with_diagnostics(
    src: &str,
    options: &CompileOptions,
) -> Result<(WorkflowDefinition, Vec<Diagnostic>), RexRapError> {
    let (mut definitions, diagnostics) = compile_all_str_with_diagnostics(src, options)?;
    if definitions.len() != 1 {
        return Err(RexRapError::Parse(format!(
            "expected exactly one workflow, found {}",
            definitions.len()
        )));
    }
    Ok((definitions.remove(0), diagnostics))
}

pub fn compile_all_str(
    src: &str,
    options: &CompileOptions,
) -> Result<Vec<WorkflowDefinition>, RexRapError> {
    compile_all_str_with_diagnostics(src, options).map(|(definitions, _)| definitions)
}

pub fn compile_all_str_with_diagnostics(
    src: &str,
    options: &CompileOptions,
) -> Result<(Vec<WorkflowDefinition>, Vec<Diagnostic>), RexRapError> {
    let mut document = parse_document(src)?;
    // resolve namespaced calls to their bare runtime form before any later pass runs.
    namespace::resolve(&mut document)?;
    // desugar a clone so sema validates the fully-expanded program, while lowering keeps the
    // sugared form to record `...alias` spreads for the decompile sidecar.
    let mut desugared = document.clone();
    desugar::desugar(&mut desugared)?;
    // the synthetic `functions.<pkg>` providers are derived from the same catalog lowering binds
    // against, so the type checker cannot accept a call lowering will then fail to resolve.
    let providers = options.all_providers();
    let diagnostics = sema::analyze_with_options(
        &desugared,
        &providers,
        options.type_policy,
        &options.workflow_signatures,
    );
    if let Some(error) = sema::first_error(&diagnostics) {
        return Err(RexRapError::semantic(error.span, error.message.clone()));
    }
    let definitions = lower::lower_document(&document, options)?;
    for definition in &definitions {
        validate(definition)?;
    }
    let warnings = diagnostics
        .into_iter()
        .filter(|diagnostic| !diagnostic.is_error())
        .collect();
    Ok((definitions, warnings))
}

/// parse and run every semantic pass, returning **all** diagnostics (errors and warnings)
/// so tooling can render the full set rather than just the first error. a parse failure
/// still surfaces as `RexRapError::Parse`. Each `Diagnostic` can be rendered against the source
/// with `Diagnostic::render`.
pub fn analyze_source(src: &str) -> Result<Vec<Diagnostic>, RexRapError> {
    analyze_source_with_providers(src, &[])
}

/// parse and run every semantic pass with provider metadata available for action result typing.
pub fn analyze_source_with_providers(
    src: &str,
    providers: &[ProviderMetadata],
) -> Result<Vec<Diagnostic>, RexRapError> {
    analyze_source_with_options(src, providers, TypePolicy::Strict, &[])
}

pub fn analyze_source_with_options(
    src: &str,
    providers: &[ProviderMetadata],
    type_policy: TypePolicy,
    workflow_signatures: &[WorkflowSignature],
) -> Result<Vec<Diagnostic>, RexRapError> {
    let mut document = parse_document(src)?;
    namespace::resolve(&mut document)?;
    desugar::desugar(&mut document)?;
    Ok(sema::analyze_with_options(
        &document,
        providers,
        type_policy,
        workflow_signatures,
    ))
}

/// parse rexrap source and render it with canonical whitespace and indentation.
pub fn format_str(src: &str) -> Result<String, RexRapError> {
    let document = parse_document(src)?;
    Ok(format::format_document(&document))
}

/// parse and lower a standalone REXRAP fragment into the runtime JSON expression/condition/program
/// shape used by the reducer.
pub fn lower_fragment(
    src: &str,
    kind: RexRapFragmentKind,
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    match kind {
        RexRapFragmentKind::Expression => {
            let mut expr = parse_expression_fragment(src)?;
            namespace::resolve_expr_fragment(&mut expr)?;
            lower::lower_expression_fragment(&expr, options)
        }
        RexRapFragmentKind::Condition => {
            let mut cond = parse_condition_fragment(src)?;
            namespace::resolve_cond_fragment(&mut cond)?;
            lower::lower_condition_fragment(&cond, options)
        }
        RexRapFragmentKind::Do => {
            let mut body = parse_do_fragment(src)?;
            namespace::resolve_compute_fragment(&mut body)?;
            lower::lower_do_fragment(&body, options)
        }
    }
}

/// validate a standalone REXRAP fragment after lowering, using the shared workflow runtime parsers.
pub fn validate_fragment(
    src: &str,
    kind: RexRapFragmentKind,
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let lowered = lower_fragment(src, kind, options)?;
    validate_lowered_fragment(&lowered, kind)?;
    Ok(lowered)
}

/// evaluate a standalone REXRAP fragment against a sample runtime context.
pub fn evaluate_fragment(
    src: &str,
    kind: RexRapFragmentKind,
    context: &Value,
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let lowered = validate_fragment(src, kind, options)?;
    match kind {
        RexRapFragmentKind::Expression => {
            runinator_workflows::resolve_value_refs_pure(&lowered, context)
                .map_err(|err| RexRapError::Validation(err.to_string()))
        }
        RexRapFragmentKind::Condition => runinator_workflows::evaluate_condition(&lowered, context)
            .map(Value::Bool)
            .map_err(|err| RexRapError::Validation(err.to_string())),
        RexRapFragmentKind::Do => {
            let program = runinator_workflows::parse_program(&lowered)
                .map_err(|err| RexRapError::Validation(err.to_string()))?;
            let catalog = runinator_workflows::CallableCatalog::builtin();
            let module = runinator_models::invocation::InvocationModule::new(
                runinator_workflows::assemble_program(&program, &catalog)
                    .map_err(|err| RexRapError::Validation(err.to_string()))?,
            );
            Ok(compute_step_value(runinator_workflows::start(
                &module,
                &runinator_workflows::VmEnv::pure(context, &catalog),
            )))
        }
    }
}

/// Validate and evaluate a pure console fragment with definitions supplied by the session library.
///
/// Fragment lowering normally has no document-level callable registry, which is correct for the
/// editor's isolated preview but insufficient for a notebook.  Build a tiny ordinary workflow
/// instead: it exercises the normal namespace, semantic, lowering, and invocation-module paths,
/// then runs that module in the same pure VM used by the regular fragment evaluator.  A `task fn`
/// call is consequently rejected by semantic purity analysis and the console classifier routes it
/// to a durable scratch workflow instead.
pub fn evaluate_fragment_with_functions(
    src: &str,
    kind: RexRapFragmentKind,
    context: &Value,
    function_sources: &[String],
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let definition = compile_console_fragment(src, kind, function_sources, options)?;
    let graph = definition.definition.as_value();
    let module_value = graph
        .get("nodes")
        .and_then(Value::as_array)
        .and_then(|nodes| {
            nodes.iter().find_map(|node| {
                (node.get("kind").and_then(Value::as_str) == Some("invocation"))
                    .then(|| {
                        node.get("parameters")
                            .and_then(|parameters| parameters.get("module"))
                    })
                    .flatten()
            })
        })
        .ok_or_else(|| RexRapError::lower("console fragment has no invocation module"))?;
    let module = module_value.decode().map_err(|err| {
        RexRapError::lower(format!(
            "console fragment invocation module is invalid: {err}"
        ))
    })?;
    let catalog = runinator_workflows::CallableCatalog::builtin();
    let step = runinator_workflows::start(
        &module,
        &runinator_workflows::VmEnv::pure(context, &catalog),
    );
    match kind {
        RexRapFragmentKind::Expression => match step {
            runinator_models::invocation::InvocationStep::Complete { value } => Ok(value),
            runinator_models::invocation::InvocationStep::Failed { message } => {
                Err(RexRapError::Validation(message))
            }
            runinator_models::invocation::InvocationStep::Yield { effect, .. } => {
                Err(RexRapError::Validation(format!(
                    "'{}' cannot be called in a pure console expression",
                    effect.target.display_name()
                )))
            }
            runinator_models::invocation::InvocationStep::Goto { target } => Err(
                RexRapError::Validation(format!("console expression continued to '{target}'")),
            ),
        },
        RexRapFragmentKind::Condition => match step {
            runinator_models::invocation::InvocationStep::Complete {
                value: Value::Bool(value),
            } => Ok(Value::Bool(value)),
            runinator_models::invocation::InvocationStep::Complete { .. } => Err(
                RexRapError::Validation("console condition did not return a boolean".into()),
            ),
            runinator_models::invocation::InvocationStep::Failed { message } => {
                Err(RexRapError::Validation(message))
            }
            runinator_models::invocation::InvocationStep::Yield { effect, .. } => {
                Err(RexRapError::Validation(format!(
                    "'{}' cannot be called in a pure console condition",
                    effect.target.display_name()
                )))
            }
            runinator_models::invocation::InvocationStep::Goto { target } => Err(
                RexRapError::Validation(format!("console condition continued to '{target}'")),
            ),
        },
        RexRapFragmentKind::Do => Ok(compute_step_value(step)),
    }
}

/// Validate the same function-aware console fragment without executing it.  The classifier uses
/// this as its proof that an expression can remain in-process.
pub fn validate_fragment_with_functions(
    src: &str,
    kind: RexRapFragmentKind,
    function_sources: &[String],
    options: &CompileOptions,
) -> Result<(), RexRapError> {
    compile_console_fragment(src, kind, function_sources, options).map(|_| ())
}

fn compile_console_fragment(
    src: &str,
    kind: RexRapFragmentKind,
    function_sources: &[String],
    options: &CompileOptions,
) -> Result<WorkflowDefinition, RexRapError> {
    if let Some(name) = task_function_called_by(src, function_sources) {
        return Err(RexRapError::Validation(format!(
            "task function '{name}' cannot be called in a pure console fragment"
        )));
    }
    let declarations = function_sources.join("\n\n");
    let compute = match kind {
        RexRapFragmentKind::Expression | RexRapFragmentKind::Condition => {
            format!("compute {{ return ({src}) }}")
        }
        RexRapFragmentKind::Do => format!("compute {src}"),
    };
    let source = format!(
        "{declarations}\nnamespace runinator.console {{\nworkflow \"__console_fragment__\" v1 {{\n    key console_fragment\n    do {{\n        {compute}\n    }}\n}}\n}}\n"
    );
    compile_str(&source, options)
}

// `task fn`s lower by inlining into a workflow graph rather than entering the pure function
// table. This guard sits at the standalone-fragment boundary, before the synthetic workflow could
// hide that distinction inside a compute node. A conservative textual match is intentional: a
// false positive merely chooses the durable workflow route, whereas a false negative could try to
// execute an effectful region in process.
fn task_function_called_by(src: &str, function_sources: &[String]) -> Option<String> {
    function_sources.iter().find_map(|source| {
        function_definitions(source)
            .ok()
            .into_iter()
            .flatten()
            .find(|function| function.is_task && contains_call(src, &function.name))
            .map(|function| function.name)
    })
}

fn contains_call(src: &str, name: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = src[start..].find(name) {
        let at = start + offset;
        let before = src[..at].chars().next_back();
        let after = src[at + name.len()..].chars().next();
        let left_boundary = before.is_none_or(|character| !is_identifier_character(character));
        if left_boundary {
            let remainder = &src[at + name.len()..];
            if remainder.trim_start().starts_with('(') {
                return true;
            }
        }
        if after.is_none() {
            break;
        }
        start = at + name.len();
    }
    false
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn validate_lowered_fragment(value: &Value, kind: RexRapFragmentKind) -> Result<(), RexRapError> {
    match kind {
        RexRapFragmentKind::Expression => runinator_workflows::validate_expression(value),
        RexRapFragmentKind::Condition => runinator_workflows::validate_condition_value(value),
        RexRapFragmentKind::Do => runinator_workflows::parse_program(value).map(|_| ()),
    }
    .map_err(|err| RexRapError::Validation(err.to_string()))
}

fn compute_step_value(step: runinator_models::invocation::InvocationStep) -> Value {
    let mut map = Map::new();
    match step {
        runinator_models::invocation::InvocationStep::Complete { value } => {
            map.insert("outcome".into(), Value::String("return".into()));
            map.insert("value".into(), value);
        }
        runinator_models::invocation::InvocationStep::Goto { target } => {
            map.insert("outcome".into(), Value::String("goto".into()));
            map.insert("target".into(), Value::String(target));
        }
        runinator_models::invocation::InvocationStep::Failed { message } => {
            map.insert("outcome".into(), Value::String("failed".into()));
            map.insert("message".into(), Value::String(message));
        }
        runinator_models::invocation::InvocationStep::Yield { effect, .. } => {
            map.insert("outcome".into(), Value::String("yield".into()));
            map.insert("target".into(), Value::String(effect.target.display_name()));
        }
    }
    Value::Object(map)
}

/// compile without running the shared validator. useful for diagnostics tooling that
/// wants to inspect partially valid output.
pub fn compile_unchecked(
    src: &str,
    options: &CompileOptions,
) -> Result<WorkflowDefinition, RexRapError> {
    let mut document = parse_document(src)?;
    namespace::resolve(&mut document)?;
    // validate the alias expansion on a clone, then lower the sugared form (see above).
    let mut desugared = document.clone();
    desugar::desugar(&mut desugared)?;
    let mut definitions = lower::lower_document(&document, options)?;
    if definitions.len() != 1 {
        return Err(RexRapError::Parse(format!(
            "expected exactly one workflow, found {}",
            definitions.len()
        )));
    }
    Ok(definitions.remove(0))
}

/// run the shared workflow validator over a definition, surfacing failures as RexRapError.
pub fn validate(definition: &WorkflowDefinition) -> Result<(), RexRapError> {
    runinator_workflows::validate_workflow(definition)
        .map(|_| ())
        .map_err(|err| RexRapError::Validation(err.to_string()))
}

/// decompile a WorkflowDefinition back into terse rexrap source text, rendered with the same
/// canonical whitespace the formatter produces. the editor regenerates this view on every
/// refresh, so routing it through the formatter keeps `format` idempotent against it: without
/// this the decompiler's inline rendering (e.g. a one-line struct type) would silently revert
/// a user's `Format` on the next refresh/save.
pub fn decompile(definition: &WorkflowDefinition) -> Result<String, RexRapError> {
    let source = decompile::decompile_definition(definition, &DecompileOptions::default())?;
    // decompiler output is always valid rexrap, so a parse failure here is a bug, not user input;
    // fall back to the raw rendering rather than failing the decompile outright.
    Ok(format_str(&source).unwrap_or(source))
}

/// decompile a definition and return the rexrap text together with the span of each graph node
/// *within that text*.
///
/// spans cannot come from the decompiler itself: [`decompile`] reflows its output through the
/// formatter, so any offset captured while rendering is stale by the time the caller sees it. they
/// also cannot be frozen into the compiled module, because the authoring source is never persisted
/// — the editor pane is this function's output, not what someone originally typed. so the text is
/// produced first and the spans are read back off it, by parsing and lowering the finished text.
/// round-tripping is what makes the node ids line up: the lowerer reproduces the same ids from the
/// same text.
pub fn decompile_with_spans(
    definition: &WorkflowDefinition,
) -> Result<(String, Vec<NodeSpan>), RexRapError> {
    let text = decompile(definition)?;
    let document = parse_document(&text)?;
    let lowered = lower::lower_document_with_spans(&document, &CompileOptions::default())?;
    // one workflow in, one workflow out; an empty document would mean decompile emitted something
    // that is not a workflow, which is a bug rather than user input.
    let spans = lowered
        .into_iter()
        .next()
        .map(|(_, spans)| spans)
        .unwrap_or_default();
    Ok((text, spans))
}

/// decompile with explicit options. `DecompileOptions { explicit: true }` renders the canonical
/// fully-expanded form (start edge, ids and happy-path arrows on every node, all defaults shown).
pub fn decompile_with(
    definition: &WorkflowDefinition,
    options: &DecompileOptions,
) -> Result<String, RexRapError> {
    decompile::decompile_definition(definition, options)
}

#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod tests;
