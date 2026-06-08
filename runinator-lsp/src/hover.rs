//! hover shows the wdl error-dictionary code and summary for the tightest diagnostic covering the
//! cursor, plus its message.

use runinator_wdl::errors::DICTIONARY;
use runinator_wdl::{Diagnostic as WdlDiagnostic, WdlError, analyze_source};
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::position::position_to_byte;

pub fn hover(text: &str, position: Position) -> Option<Hover> {
    let offset = position_to_byte(text, position);
    let (code, message) = match analyze_source(text) {
        Ok(diagnostics) => {
            let diagnostic = tightest(&diagnostics, offset)?;
            ("WDL003", diagnostic.message.clone())
        }
        Err(error) => match error_at(&error, offset) {
            Some(entry) => entry,
            None => return None,
        },
    };
    Some(markdown(code, &message))
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

fn markdown(code: &str, message: &str) -> Hover {
    let summary = DICTIONARY
        .iter()
        .find(|descriptor| descriptor.code == code)
        .map(|descriptor| descriptor.summary)
        .unwrap_or("Diagnostic");
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**{code} — {summary}**\n\n{message}"),
        }),
        range: None,
    }
}
