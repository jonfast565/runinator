// runinator-wdl: a human-friendly workflow language that transpiles to the existing
// runinator json workflow model. parse wdl text -> ast -> WorkflowDefinition, and
// decompile a WorkflowDefinition back to wdl text. the runtime is unchanged; this crate
// is purely an author-time front end.

use runinator_models::semver::SemVer;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowDefinition;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod ast;
pub mod completion;
mod decompile;
mod desugar;
pub mod errors;
mod format;
mod includes;
pub(crate) mod lower;
mod parser;
mod purity;
mod registry;
mod secrets;
pub mod sema;

pub use decompile::DecompileOptions;
pub use errors::{Span, WdlError};
pub use includes::included_file_paths;
pub use parser::{
    parse_compute_fragment, parse_condition_fragment, parse_document, parse_expression_fragment,
};
pub use secrets::{parse_secrets_str, secrets_to_wdls};
pub use sema::{Diagnostic, Severity};

pub use completion::{
    WdlCompletionItem, WdlCompletionRequest, WdlCompletionResponse, complete_source,
};

/// options that fill in the WorkflowDefinition fields that the source does not carry.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub enabled: bool,
    /// fallback version when the source omits `vN`.
    pub default_version: SemVer,
    /// directory used to resolve `file("...")` includes.
    pub source_dir: Option<PathBuf>,
}

/// the supported standalone WDL fragment surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WdlFragmentKind {
    Expression,
    Condition,
    Compute,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            default_version: SemVer::default(),
            source_dir: None,
        }
    }
}

/// compile wdl source into a validated WorkflowDefinition. semantic errors block the
/// compile; warnings are dropped (use `compile_str_with_diagnostics` to inspect them).
pub fn compile_str(src: &str, options: &CompileOptions) -> Result<WorkflowDefinition, WdlError> {
    compile_str_with_diagnostics(src, options).map(|(definition, _)| definition)
}

/// like `compile_str`, but also returns the advisory (warning) diagnostics. semantic errors
/// still short-circuit with `WdlError::Semantic`.
pub fn compile_str_with_diagnostics(
    src: &str,
    options: &CompileOptions,
) -> Result<(WorkflowDefinition, Vec<Diagnostic>), WdlError> {
    let document = parse_document(src)?;
    // desugar a clone so sema validates the fully-expanded program, while lowering keeps the
    // sugared form to record `...alias` spreads for the decompile sidecar.
    let mut desugared = document.clone();
    desugar::desugar(&mut desugared)?;
    let diagnostics = sema::analyze(&desugared);
    if let Some(error) = sema::first_error(&diagnostics) {
        return Err(WdlError::semantic(error.span, error.message.clone()));
    }
    let definition = lower::lower_document(&document, options)?;
    validate(&definition)?;
    let warnings = diagnostics
        .into_iter()
        .filter(|diagnostic| !diagnostic.is_error())
        .collect();
    Ok((definition, warnings))
}

/// parse and run every semantic pass, returning **all** diagnostics (errors and warnings)
/// so tooling can render the full set rather than just the first error. a parse failure
/// still surfaces as `WdlError::Parse`. Each `Diagnostic` can be rendered against the source
/// with `Diagnostic::render`.
pub fn analyze_source(src: &str) -> Result<Vec<Diagnostic>, WdlError> {
    let mut document = parse_document(src)?;
    desugar::desugar(&mut document)?;
    Ok(sema::analyze(&document))
}

/// parse wdl source and render it with canonical whitespace and indentation.
pub fn format_str(src: &str) -> Result<String, WdlError> {
    let document = parse_document(src)?;
    Ok(format::format_document(&document))
}

/// parse and lower a standalone WDL fragment into the runtime JSON expression/condition/program
/// shape used by the reducer.
pub fn lower_fragment(
    src: &str,
    kind: WdlFragmentKind,
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    match kind {
        WdlFragmentKind::Expression => {
            let expr = parse_expression_fragment(src)?;
            lower::lower_expression_fragment(&expr, options)
        }
        WdlFragmentKind::Condition => {
            let cond = parse_condition_fragment(src)?;
            lower::lower_condition_fragment(&cond, options)
        }
        WdlFragmentKind::Compute => {
            let body = parse_compute_fragment(src)?;
            lower::lower_compute_fragment(&body, options)
        }
    }
}

/// validate a standalone WDL fragment after lowering, using the shared workflow runtime parsers.
pub fn validate_fragment(
    src: &str,
    kind: WdlFragmentKind,
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    let lowered = lower_fragment(src, kind, options)?;
    validate_lowered_fragment(&lowered, kind)?;
    Ok(lowered)
}

/// evaluate a standalone WDL fragment against a sample runtime context.
pub fn evaluate_fragment(
    src: &str,
    kind: WdlFragmentKind,
    context: &Value,
    options: &CompileOptions,
) -> Result<Value, WdlError> {
    let lowered = validate_fragment(src, kind, options)?;
    match kind {
        WdlFragmentKind::Expression => {
            runinator_workflows::resolve_value_refs_pure(&lowered, context)
                .map_err(|err| WdlError::Validation(err.to_string()))
        }
        WdlFragmentKind::Condition => runinator_workflows::evaluate_condition(&lowered, context)
            .map(Value::Bool)
            .map_err(|err| WdlError::Validation(err.to_string())),
        WdlFragmentKind::Compute => {
            let program = runinator_workflows::parse_program(&lowered)
                .map_err(|err| WdlError::Validation(err.to_string()))?;
            let outcome = runinator_workflows::run_program(
                &program,
                context,
                &runinator_workflows::PureIntrinsics,
            )
            .map_err(|err| WdlError::Validation(err.to_string()))?;
            Ok(compute_outcome_value(outcome))
        }
    }
}

fn validate_lowered_fragment(value: &Value, kind: WdlFragmentKind) -> Result<(), WdlError> {
    match kind {
        WdlFragmentKind::Expression => runinator_workflows::validate_expression(value),
        WdlFragmentKind::Condition => runinator_workflows::validate_condition_value(value),
        WdlFragmentKind::Compute => runinator_workflows::parse_program(value).map(|_| ()),
    }
    .map_err(|err| WdlError::Validation(err.to_string()))
}

fn compute_outcome_value(outcome: runinator_workflows::ComputeOutcome) -> Value {
    let mut map = Map::new();
    match outcome {
        runinator_workflows::ComputeOutcome::Return(value) => {
            map.insert("outcome".into(), Value::String("return".into()));
            map.insert("value".into(), value);
        }
        runinator_workflows::ComputeOutcome::Goto(target) => {
            map.insert("outcome".into(), Value::String("goto".into()));
            map.insert("target".into(), Value::String(target));
        }
        runinator_workflows::ComputeOutcome::Fallthrough(value) => {
            map.insert("outcome".into(), Value::String("fallthrough".into()));
            map.insert("value".into(), value);
        }
    }
    Value::Object(map)
}

/// compile without running the shared validator. useful for diagnostics tooling that
/// wants to inspect partially valid output.
pub fn compile_unchecked(
    src: &str,
    options: &CompileOptions,
) -> Result<WorkflowDefinition, WdlError> {
    let document = parse_document(src)?;
    // validate the alias expansion on a clone, then lower the sugared form (see above).
    let mut desugared = document.clone();
    desugar::desugar(&mut desugared)?;
    lower::lower_document(&document, options)
}

/// run the shared workflow validator over a definition, surfacing failures as WdlError.
pub fn validate(definition: &WorkflowDefinition) -> Result<(), WdlError> {
    runinator_workflows::validate_workflow(definition)
        .map(|_| ())
        .map_err(|err| WdlError::Validation(err.to_string()))
}

/// decompile a WorkflowDefinition back into terse wdl source text.
pub fn decompile(definition: &WorkflowDefinition) -> Result<String, WdlError> {
    decompile::decompile_definition(definition, &DecompileOptions::default())
}

/// decompile with explicit options. `DecompileOptions { explicit: true }` renders the canonical
/// fully-expanded form (start edge, ids and happy-path arrows on every node, all defaults shown).
pub fn decompile_with(
    definition: &WorkflowDefinition,
    options: &DecompileOptions,
) -> Result<String, WdlError> {
    decompile::decompile_definition(definition, options)
}

#[cfg(test)]
mod tests;
