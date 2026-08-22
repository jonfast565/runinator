//! a byte position inside a rexrap source buffer, plus the lexical scanning completion and hover both
//! need to answer "what token/path/paren context sits at this position". `source` never changes
//! within one completion/hover request, only `pos` does (a token start, the char before a word, an
//! unmatched paren, ...), so the pair is a `Copy` value passed around rather than re-threaded as two
//! arguments everywhere.

use crate::completion::{
    ActionCallContext, ActionMemberContext, CompletionSpanContext, PathContext,
};
use crate::hover::{HoverPath, WordAt};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Cursor<'a> {
    pub(crate) source: &'a str,
    pub(crate) pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(source: &'a str, pos: usize) -> Self {
        Self { source, pos }
    }

    /// a cursor over the same source at a different position.
    pub(crate) fn at(self, pos: usize) -> Self {
        Self {
            source: self.source,
            pos,
        }
    }
}

pub(crate) fn clamp_to_char_boundary(source: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(source.len());
    while cursor > 0 && !source.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

impl<'a> Cursor<'a> {
    pub(crate) fn current_word(&self) -> WordBounds {
        let mut start = self.pos;
        while start > 0 {
            let byte = self.source.as_bytes()[start - 1];
            if !is_ident_continue(byte) {
                break;
            }
            start -= 1;
        }
        WordBounds { start }
    }

    pub(crate) fn current_path_bounds(&self) -> (usize, usize) {
        let mut start = self.pos;
        while start > 0 {
            let byte = self.source.as_bytes()[start - 1];
            if !(is_ident_continue(byte) || byte == b'.') {
                break;
            }
            start -= 1;
        }
        (start, self.pos)
    }

    pub(crate) fn previous_non_space(&self) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut index = self.pos;
        while index > 0 {
            index -= 1;
            if !bytes[index].is_ascii_whitespace() {
                return Some(index);
            }
        }
        None
    }

    pub(crate) fn next_non_space(&self) -> Option<usize> {
        let mut index = self.pos;
        let bytes = self.source.as_bytes();
        while index < bytes.len() {
            if !bytes[index].is_ascii_whitespace() {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    pub(crate) fn unmatched_open_paren(&self) -> Option<usize> {
        let mut depth = 0usize;
        for (index, ch) in self.source[..self.pos].char_indices().rev() {
            match ch {
                ')' => depth += 1,
                '(' if depth == 0 => return Some(index),
                '(' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }

    pub(crate) fn is_completion_allowed(&self) -> bool {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum State {
            Normal,
            LineComment,
            BlockComment,
            String,
            Interpolation(usize),
        }

        let bytes = self.source.as_bytes();
        let mut state = State::Normal;
        let mut index = 0;
        let mut escaped = false;
        while index < self.pos {
            let byte = bytes[index];
            let next = bytes.get(index + 1).copied();
            match state {
                State::Normal => {
                    if byte == b'/' && next == Some(b'/') {
                        state = State::LineComment;
                        index += 2;
                        continue;
                    }
                    if byte == b'/' && next == Some(b'*') {
                        state = State::BlockComment;
                        index += 2;
                        continue;
                    }
                    if byte == b'"' {
                        state = State::String;
                        escaped = false;
                    }
                }
                State::LineComment => {
                    if byte == b'\n' {
                        state = State::Normal;
                    }
                }
                State::BlockComment => {
                    if byte == b'*' && next == Some(b'/') {
                        state = State::Normal;
                        index += 2;
                        continue;
                    }
                }
                State::String => {
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'$' && next == Some(b'{') {
                        state = State::Interpolation(1);
                        index += 2;
                        continue;
                    } else if byte == b'"' {
                        state = State::Normal;
                    }
                }
                State::Interpolation(depth) => {
                    if byte == b'{' {
                        state = State::Interpolation(depth + 1);
                    } else if byte == b'}' {
                        if depth <= 1 {
                            state = State::String;
                        } else {
                            state = State::Interpolation(depth - 1);
                        }
                    }
                }
            }
            index += 1;
        }
        matches!(state, State::Normal | State::Interpolation(_))
    }

    pub(crate) fn transition_target_context(&self) -> Option<CompletionSpanContext> {
        let word = self.current_word();
        let before_word = &self.source[..word.start];
        // `continue <target>` in a route arm, `goto <target>` in a compute block, and the `->`
        // targets the header declarations still use.
        if before_word.trim_end().ends_with("->")
            || matches!(previous_word(before_word), Some("goto") | Some("continue"))
        {
            return Some(CompletionSpanContext {
                replace_start: word.start,
            });
        }
        None
    }

    pub(crate) fn edge_outcome_context(&self) -> Option<CompletionSpanContext> {
        let word = self.current_word();
        if self.transition_target_context().is_some() {
            return None;
        }
        if self.at(word.start).inside_edges_block() {
            return Some(CompletionSpanContext {
                replace_start: word.start,
            });
        }

        let line_start = self.source[..word.start]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        let prefix = self.source[line_start..word.start].trim_end();
        if prefix.is_empty() || prefix.ends_with("->") {
            return None;
        }
        let trimmed = prefix.trim_start();
        if trimmed.starts_with("node ") && completed_statement_prefix(trimmed) {
            return Some(CompletionSpanContext {
                replace_start: word.start,
            });
        }
        None
    }

    /// true when the cursor sits directly in a `routes { … }` section — where a route arm head
    /// (`on success`, `when …`) goes — rather than inside one of its arm bodies.
    pub(crate) fn inside_edges_block(&self) -> bool {
        let Some(edges_start) = self.source[..self.pos].rfind("routes") else {
            return false;
        };
        if !is_keyword_at(self.source, edges_start, "routes") {
            return false;
        }
        let Some(open_rel) = self.source[edges_start..self.pos].find('{') else {
            return false;
        };
        let open = edges_start + open_rel;
        let mut depth = 0usize;
        for byte in self.source[open..self.pos].bytes() {
            match byte {
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        depth > 0
    }

    pub(crate) fn path_context(&self) -> Option<PathContext> {
        let (start, end) = self.current_path_bounds();
        if start == end {
            return None;
        }
        let token = &self.source[start..end];
        if !token.contains('.') {
            return None;
        }
        let mut parts = token.split('.').collect::<Vec<_>>();
        if parts.is_empty() || parts[0].is_empty() {
            return None;
        }
        let partial = parts.pop().unwrap_or_default();
        let completed = parts
            .iter()
            .skip(1)
            .filter(|part| !part.is_empty())
            .map(|part| (*part).to_string())
            .collect::<Vec<_>>();
        let replace_start = end.saturating_sub(partial.len());
        Some(PathContext {
            head: parts[0].to_string(),
            completed,
            replace_start,
            replace_end: self.pos,
        })
    }

    pub(crate) fn action_member_context(&self) -> Option<ActionMemberContext> {
        let word_start = self.current_word().start;
        let dot = self.at(word_start).previous_non_space()?;
        if self.source.as_bytes().get(dot) != Some(&b'.') {
            return None;
        }
        let provider_end = dot;
        // the whole dotted prefix, not one segment. there is no guard here against a dotted *value*
        // path (`config.database.<cursor>`): the caller only uses this when the name resolves to a
        // real provider, and that lookup is a better discriminator than a syntactic rule that also
        // rejected every legitimate multi-segment provider.
        let provider_start = dotted_identifier_start_before(self.source, provider_end)?;
        let provider = self.source[provider_start..provider_end].to_string();
        Some(ActionMemberContext {
            provider,
            replace_start: word_start,
        })
    }

    pub(crate) fn action_call_context(&self) -> Option<ActionCallContext> {
        let open = self.unmatched_open_paren()?;
        let before_open = self.source[..open].trim_end();
        let dot = before_open.rfind('.')?;
        let action_start = identifier_start_before(before_open, before_open.len())?;
        if action_start <= dot {
            return None;
        }
        let provider_end = dot;
        let provider_start = dotted_identifier_start_before(before_open, provider_end)?;
        let provider = before_open[provider_start..provider_end].to_string();
        let action = before_open[action_start..before_open.len()].to_string();
        if provider.is_empty() || action.is_empty() {
            return None;
        }
        let word = self.current_word();
        let used_args = used_argument_names(&self.source[open + 1..self.pos]);
        Some(ActionCallContext {
            provider,
            action,
            replace_start: word.start,
            replace_end: self.pos,
            used_args,
        })
    }

    pub(crate) fn word_at(&self) -> Option<WordAt<'a>> {
        self.token_at(false)
    }

    pub(crate) fn action_token_at(&self) -> Option<WordAt<'a>> {
        let token = self.token_at(true)?;
        token.text.contains('.').then_some(token)
    }

    pub(crate) fn token_at(&self, allow_dot_and_hyphen: bool) -> Option<WordAt<'a>> {
        let mut cursor = clamp_to_char_boundary(self.source, self.pos);
        let bytes = self.source.as_bytes();
        if cursor == self.source.len() && cursor > 0 {
            cursor -= 1;
        }
        if bytes
            .get(cursor)
            .is_none_or(|byte| !token_continue(*byte, allow_dot_and_hyphen))
        {
            if cursor == 0 || !token_continue(bytes[cursor - 1], allow_dot_and_hyphen) {
                return None;
            }
            cursor -= 1;
        }
        let mut start = cursor;
        while start > 0 && token_continue(bytes[start - 1], allow_dot_and_hyphen) {
            start -= 1;
        }
        let mut end = cursor + 1;
        while end < bytes.len() && token_continue(bytes[end], allow_dot_and_hyphen) {
            end += 1;
        }
        let text = &self.source[start..end];
        (!text.is_empty()).then_some(WordAt { text, start, end })
    }

    pub(crate) fn path_at(&self) -> Option<HoverPath<'a>> {
        let token = self.token_at(true)?;
        if !token.text.contains('.') {
            return None;
        }
        let _ = self.at(token.end).path_context()?;
        let mut parts = Vec::new();
        let mut ranges = Vec::new();
        let mut part_start = 0usize;
        for (index, ch) in token.text.char_indices() {
            if ch != '.' {
                continue;
            }
            if part_start < index {
                parts.push(&token.text[part_start..index]);
                ranges.push((token.start + part_start, token.start + index));
            }
            part_start = index + 1;
        }
        if part_start < token.text.len() {
            parts.push(&token.text[part_start..]);
            ranges.push((token.start + part_start, token.end));
        }
        (parts.len() > 1).then_some(HoverPath { parts, ranges })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WordBounds {
    pub(crate) start: usize,
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
