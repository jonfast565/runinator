//! compute lsp diagnostics for a wdl document by reusing the wdl crate's analyzer and compiler.

use std::path::Path;

use runinator_models::semver::SemVer;
use runinator_wdl::{
    CompileOptions, Diagnostic as WdlDiagnostic, Severity, WdlError, analyze_source_with_options,
    compile_str_with_diagnostics,
};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::position::{span_to_range, whole_document_range};

/// analyze `text` and, when `check_lowering` is set (on save), also attempt a full compile to
/// surface lowering/validation errors that analysis alone does not catch.
pub fn compute(text: &str, path: Option<&Path>, check_lowering: bool) -> Vec<Diagnostic> {
    let providers = runinator_provider_catalog::metadata();
    let workflow_signatures = path
        .and_then(|path| {
            runinator_pack::source::wdl_context_workflow_signatures(path, Some(text)).ok()
        })
        .unwrap_or_default();
    match analyze_source_with_options(
        text,
        &providers,
        runinator_wdl::TypePolicy::Strict,
        &workflow_signatures,
    ) {
        Ok(diagnostics) => {
            let mut out: Vec<Diagnostic> = diagnostics.iter().map(|d| from_wdl(text, d)).collect();
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

fn from_wdl(text: &str, diagnostic: &WdlDiagnostic) -> Diagnostic {
    Diagnostic {
        range: span_to_range(text, diagnostic.span),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        source: Some("wdl".to_string()),
        message: diagnostic.message.clone(),
        ..Default::default()
    }
}

fn from_error(text: &str, error: &WdlError) -> Diagnostic {
    let (range, code) = match error {
        WdlError::Syntax { span, .. } => (span_to_range(text, *span), "WDL002"),
        WdlError::Semantic { span, .. } => (span_to_range(text, *span), "WDL003"),
        WdlError::Parse(_) => (whole_document_range(text), "WDL001"),
        WdlError::Lower(_) => (whole_document_range(text), "WDL004"),
        WdlError::Validation(_) => (whole_document_range(text), "WDL005"),
        WdlError::Decompile(_) => (whole_document_range(text), "WDL006"),
    };
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(code.to_string())),
        source: Some("wdl".to_string()),
        message: error.to_string(),
        ..Default::default()
    }
}
