// runinator-wdl: a human-friendly workflow language that transpiles to the existing
// runinator json workflow model. parse wdl text -> ast -> WorkflowDefinition, and
// decompile a WorkflowDefinition back to wdl text. the runtime is unchanged; this crate
// is purely an author-time front end.

use runinator_models::semver::SemVer;
use runinator_models::workflows::WorkflowDefinition;

pub mod ast;
pub mod completion;
mod decompile;
mod desugar;
pub mod errors;
mod format;
pub(crate) mod lower;
mod parser;
mod purity;
mod secrets;
pub mod sema;

pub use decompile::DecompileOptions;
pub use errors::{Span, WdlError};
pub use parser::parse_document;
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
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            default_version: SemVer::default(),
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
