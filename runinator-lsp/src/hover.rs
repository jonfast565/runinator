//! hover shows WDL symbol docs/type information. when the document does not parse, it falls back to
//! the diagnostic under the cursor.

use std::path::Path;

use runinator_wdl::errors::DICTIONARY;
use runinator_wdl::{Diagnostic as WdlDiagnostic, WdlError, analyze_source_with_options};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use crate::position::{position_to_byte, span_to_range};

pub fn hover(text: &str, path: Option<&Path>, position: Position) -> Option<Hover> {
    let offset = position_to_byte(text, position);
    let providers = runinator_provider_catalog::metadata();
    if let Some(hover) = runinator_wdl::hover_source(runinator_wdl::WdlHoverRequest {
        source: text.to_string(),
        cursor_byte: offset,
        providers: providers.clone(),
        settings: Vec::new(),
    }) {
        let range = span_to_range(
            text,
            runinator_wdl::Span {
                start: hover.range_start_byte,
                end: hover.range_end_byte,
            },
        );
        return Some(markdown_hover(hover_markdown(&hover), Some(range)));
    }
    let workflow_signatures = path
        .and_then(|path| {
            runinator_pack::source::wdl_context_workflow_signatures(path, Some(text)).ok()
        })
        .unwrap_or_default();
    let (code, message) = match analyze_source_with_options(
        text,
        &providers,
        runinator_wdl::TypePolicy::Strict,
        &workflow_signatures,
    ) {
        Ok(diagnostics) => {
            let diagnostic = tightest(&diagnostics, offset)?;
            ("WDL003", diagnostic.message.clone())
        }
        Err(error) => match error_at(&error, offset) {
            Some(entry) => entry,
            None => return None,
        },
    };
    Some(markdown_diagnostic(code, &message))
}

// the smallest-width diagnostic whose span contains `offset`.
fn tightest(diagnostics: &[WdlDiagnostic], offset: usize) -> Option<&WdlDiagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.span.start <= offset && offset < d.span.end)
        .min_by_key(|d| d.span.end.saturating_sub(d.span.start))
}

// the dictionary code + message for a span-carrying error covering `offset`.
fn error_at(error: &WdlError, offset: usize) -> Option<(&'static str, String)> {
    match error {
        WdlError::Syntax { span, message } if span.start <= offset && offset < span.end => {
            Some(("WDL002", message.clone()))
        }
        WdlError::Semantic { span, message } if span.start <= offset && offset < span.end => {
            Some(("WDL003", message.clone()))
        }
        _ => None,
    }
}

fn markdown_diagnostic(code: &str, message: &str) -> Hover {
    let summary = DICTIONARY
        .iter()
        .find(|descriptor| descriptor.code == code)
        .map(|descriptor| descriptor.summary)
        .unwrap_or("Diagnostic");
    markdown_hover(format!("**{code} - {summary}**\n\n{message}"), None)
}

fn hover_markdown(hover: &runinator_wdl::WdlHoverResponse) -> String {
    let mut out = format!("**{}**\n\n_{}_", hover.title, hover.kind);
    if let Some(detail) = &hover.detail {
        out.push_str("\n\n```wdl\n");
        out.push_str(detail);
        out.push_str("\n```");
    }
    if let Some(documentation) = &hover.documentation {
        out.push_str("\n\n");
        out.push_str(documentation);
    }
    out
}

fn markdown_hover(value: String, range: Option<Range>) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range,
    }
}
