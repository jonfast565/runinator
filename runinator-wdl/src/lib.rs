// runinator-wdl: a human-friendly workflow language that transpiles to the existing
// runinator json workflow model. parse wdl text -> ast -> WorkflowDefinition, and
// decompile a WorkflowDefinition back to wdl text. the runtime is unchanged; this crate
// is purely an author-time front end.

use runinator_models::workflows::WorkflowDefinition;

pub mod ast;
mod decompile;
mod errors;
mod lower;
mod parser;
pub mod sema;

pub use errors::{Span, WdlError};
pub use parser::parse_document;
pub use sema::{Diagnostic, Severity};

/// options that fill in the WorkflowDefinition fields that the source does not carry.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub enabled: bool,
    /// fallback version when the source omits `vN`.
    pub default_version: i64,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            default_version: 1,
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
    let diagnostics = sema::analyze(&document);
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
    let document = parse_document(src)?;
    Ok(sema::analyze(&document))
}

/// compile without running the shared validator. useful for diagnostics tooling that
/// wants to inspect partially valid output.
pub fn compile_unchecked(
    src: &str,
    options: &CompileOptions,
) -> Result<WorkflowDefinition, WdlError> {
    let document = parse_document(src)?;
    lower::lower_document(&document, options)
}

/// run the shared workflow validator over a definition, surfacing failures as WdlError.
pub fn validate(definition: &WorkflowDefinition) -> Result<(), WdlError> {
    runinator_workflows::validate_workflow(definition)
        .map(|_| ())
        .map_err(|err| WdlError::Validation(err.to_string()))
}

/// decompile a WorkflowDefinition back into wdl source text.
pub fn decompile(definition: &WorkflowDefinition) -> Result<String, WdlError> {
    decompile::decompile_definition(definition)
}

#[cfg(test)]
mod tests;
