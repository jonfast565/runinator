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
    parse_condition_fragment, parse_do_fragment, parse_document, parse_expression_fragment,
};
pub use secrets::{parse_secrets_str, secrets_to_rexraps};
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
