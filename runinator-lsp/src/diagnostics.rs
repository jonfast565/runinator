//! compute lsp diagnostics for a rexrap document by reusing the rexrap crate's analyzer and compiler.

use std::path::Path;

use runinator_models::semver::SemVer;
use runinator_rexrap::{
    CompileOptions, Diagnostic as RexRapDiagnostic, RexRapError, Severity,
    analyze_source_with_options, compile_str_with_diagnostics,
};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::position::{span_to_range, whole_document_range};

/// analyze `text` and, when `check_lowering` is set (on save), also attempt a full compile to
/// surface lowering/validation errors that analysis alone does not catch.
pub fn compute(text: &str, path: Option<&Path>, check_lowering: bool) -> Vec<Diagnostic> {
    let providers = runinator_provider_catalog::metadata();
    let workflow_signatures = path
        .and_then(|path| {
            runinator_pack::source::rexrap_context_workflow_signatures(path, Some(text)).ok()
        })
        .unwrap_or_default();
    match analyze_source_with_options(
        text,
        &providers,
        runinator_rexrap::TypePolicy::Strict,
        &workflow_signatures,
    ) {
        Ok(diagnostics) => {
            let mut out: Vec<Diagnostic> =
                diagnostics.iter().map(|d| from_rexrap(text, d)).collect();
            if check_lowering {
                let options = CompileOptions {
                    enabled: true,
                    default_version: SemVer::default(),
                    source_dir: path.and_then(Path::parent).map(Path::to_path_buf),
                    providers: providers.clone(),
                    workflow_signatures,
                    ..CompileOptions::default()
                };
                if let Err(err) = compile_str_with_diagnostics(text, &options) {
                    out.push(from_error(text, &err));
                }
            }
            out
        }
        Err(err) => vec![from_error(text, &err)],
    }
}

fn from_rexrap(text: &str, diagnostic: &RexRapDiagnostic) -> Diagnostic {
    Diagnostic {
        range: span_to_range(text, diagnostic.span),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        source: Some("rexrap".to_string()),
        message: diagnostic.message.clone(),
        ..Default::default()
    }
}

fn from_error(text: &str, error: &RexRapError) -> Diagnostic {
    let (range, code) = match error {
        RexRapError::Syntax { span, .. } => (span_to_range(text, *span), "REXRAP002"),
        RexRapError::Semantic { span, .. } => (span_to_range(text, *span), "REXRAP003"),
        RexRapError::Parse(_) => (whole_document_range(text), "REXRAP001"),
        RexRapError::Lower(_) => (whole_document_range(text), "REXRAP004"),
        RexRapError::Validation(_) => (whole_document_range(text), "REXRAP005"),
        RexRapError::Decompile(_) => (whole_document_range(text), "REXRAP006"),
    };
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(code.to_string())),
        source: Some("rexrap".to_string()),
        message: error.to_string(),
        ..Default::default()
    }
}
