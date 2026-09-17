//! a byte position inside a rexrap source buffer, plus the lexical scanning completion and hover both
//! need to answer "what token/path/paren context sits at this position". `source` never changes
//! within one completion/hover request, only `pos` does (a token start, the char before a word, an
//! unmatched paren, ...), so the pair is a `Copy` value passed around rather than re-threaded as two
//! arguments everywhere.

use crate::completion::{
    ActionCallContext, ActionMemberContext, CompletionSpanContext, PathContext,
};
use crate::hover::{HoverPath, WordAt};

pub(crate) fn clamp_to_char_boundary(source: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(source.len());
    while cursor > 0 && !source.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn completed_statement_prefix(prefix: &str) -> bool {
    prefix.ends_with(')') || prefix.ends_with('}') || prefix.ends_with('"')
}

fn is_keyword_at(source: &str, start: usize, keyword: &str) -> bool {
    let end = start + keyword.len();
    let before_ok = start == 0 || !is_ident_continue(source.as_bytes()[start - 1]);
    let after_ok = source
        .as_bytes()
        .get(end)
        .is_none_or(|byte| !is_ident_continue(*byte));
    before_ok && after_ok
}

fn previous_word(source: &str) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut end = source.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_continue(bytes[start - 1]) {
        start -= 1;
    }
    (start < end).then_some(&source[start..end])
}

/// walk back over a dotted provider path, `ident(.ident)*`, and return where it starts.
///
/// a provider name is every segment but the last of a call, so `functions.image_tools.resize` has
/// the two-segment provider `functions.image_tools`. reading only one identifier back would report
/// the provider as `image_tools`, which matches nothing.
///
/// this deliberately does not decide *whether* the prefix is a provider — `config.database.host` is
/// walked back the same way. the caller settles that by looking the name up, which is the only test
/// that can tell a dotted provider from a dotted value path.
fn dotted_identifier_start_before(source: &str, end: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut start = identifier_start_before(source, end)?;
    while start > 0 && bytes[start - 1] == b'.' {
        let Some(previous) = identifier_start_before(source, start - 1) else {
            break;
        };
        start = previous;
    }
    Some(start)
}

fn identifier_start_before(source: &str, end: usize) -> Option<usize> {
    if end == 0 {
        return None;
    }
    let bytes = source.as_bytes();
    let mut start = end;
    while start > 0 && is_action_ident_continue(bytes[start - 1]) {
        start -= 1;
    }
    if start == end { None } else { Some(start) }
}

fn used_argument_names(text: &str) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                index += 1;
                continue;
            }
            b'(' | b'{' | b'[' => {
                depth += 1;
                index += 1;
                continue;
            }
            b')' | b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
                continue;
            }
            _ => {}
        }
        if depth == 0 && is_ident_start(byte) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_ident_continue(bytes[index]) {
                index += 1;
            }
            let mut lookahead = index;
            while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                lookahead += 1;
            }
            if bytes.get(lookahead) == Some(&b':') {
                names.insert(text[start..index].to_string());
            }
        } else {
            index += 1;
        }
    }
    names
}

fn token_continue(byte: u8, allow_dot_and_hyphen: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || byte == b'_'
        || (allow_dot_and_hyphen && (byte == b'.' || byte == b'-'))
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_action_ident_continue(byte: u8) -> bool {
    is_ident_continue(byte) || byte == b'-'
}

mod cursor;
pub(crate) use cursor::Cursor;

mod word_bounds;
pub(crate) use word_bounds::WordBounds;
