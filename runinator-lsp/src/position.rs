//! mapping between lsp positions (0-based, utf-16) and wdl byte offsets / spans.
//! the wdl crate works in byte offsets; lsp speaks utf-16 code units, so every boundary that
//! crosses between them goes through here.

use runinator_wdl::Span;
use tower_lsp::lsp_types::{Position, Range};

/// resolve a 0-based utf-16 `Position` to a byte offset into `text`, clamped to a char boundary.
pub fn position_to_byte(text: &str, position: Position) -> usize {
    let bytes = text.as_bytes();
    let mut byte = 0usize;
    let mut line = 0u32;
    // advance to the first byte of the target line.
    while line < position.line && byte < bytes.len() {
        if bytes[byte] == b'\n' {
            line += 1;
        }
        byte += 1;
    }
    // advance `character` utf-16 units within the line, stopping at the newline.
    let mut utf16 = 0u32;
    for ch in text[byte..].chars() {
        if ch == '\n' || utf16 >= position.character {
            break;
        }
        utf16 += ch.len_utf16() as u32;
        byte += ch.len_utf8();
    }
    byte.min(text.len())
}

/// resolve a byte offset into `text` to a 0-based utf-16 `Position`.
pub fn byte_to_position(text: &str, offset: usize) -> Position {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + ch.len_utf8();
        }
    }
    let character = text[line_start..offset]
        .chars()
        .map(|ch| ch.len_utf16() as u32)
        .sum();
    Position { line, character }
}

/// convert a wdl `Span` (byte offsets) to an lsp `Range`.
pub fn span_to_range(text: &str, span: Span) -> Range {
    Range {
        start: byte_to_position(text, span.start),
        end: byte_to_position(text, span.end),
    }
}

/// convert a raw byte range to an lsp `Range`.
pub fn bytes_to_range(text: &str, start: usize, end: usize) -> Range {
    Range {
        start: byte_to_position(text, start),
        end: byte_to_position(text, end),
    }
}

/// a `Range` spanning the whole document, used to anchor span-less errors.
pub fn whole_document_range(text: &str) -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: byte_to_position(text, text.len()),
    }
}

#[cfg(test)]
mod tests;
