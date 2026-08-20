//! The unified `.rrx` source container.
//!
//! A file may carry any number of the existing top-level language declarations.  `workflow` and
//! `pipeline` retain their established grammars; `settings`, `tests`, and `package` are named
//! blocks whose bodies are handed to the appropriate front end (or retained for tooling).  Keeping
//! this tiny framing layer outside the workflow grammar lets each existing compiler stay focused on
//! the construct it already owns.

use crate::{RexRapError, Span};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RrxBlocks {
    pub workflows: String,
    pub pipelines: String,
    pub settings: Vec<String>,
    pub tests: Vec<String>,
    pub packages: Vec<String>,
}

/// Split a unified source file into its typed top-level declarations.
///
/// The scanner deliberately understands only enough structure to find matching braces while
/// respecting quoted strings and line comments. The real workflow/pipeline/settings parsers still
/// validate each extracted payload and produce their normal diagnostics.
pub fn parse_rrx_blocks(source: &str) -> Result<RrxBlocks, RexRapError> {
    let mut blocks = RrxBlocks::default();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut header = String::new();

    while cursor < bytes.len() {
        cursor = skip_space_and_comments(source, cursor);
        if cursor >= bytes.len() {
            break;
        }
        let start = cursor;
        let Some((word, after_word)) = read_word(source, cursor) else {
            return Err(RexRapError::syntax(
                Span::new(cursor, cursor + 1),
                "expected a named top-level RexRap declaration",
            ));
        };
        if word == "language" {
            let end = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(source.len());
            header.push_str(&source[cursor..end]);
            cursor = end;
            continue;
        }
        let mut brace = find_open_brace(source, after_word).ok_or_else(|| {
            RexRapError::syntax(
                Span::new(start, after_word),
                format!("top-level `{word}` declaration needs a block"),
            )
        })?;
        // A workflow's optional structural type (`returns { ... }`) precedes its body. The first
        // brace is that type in this form, so advance to the following body brace.
        if word == "workflow" {
            let first_end = matching_brace(source, brace)?;
            let next = skip_space_and_comments(source, first_end);
            if source.as_bytes().get(next) == Some(&b'{') {
                brace = next;
            }
        }
        let end = matching_brace(source, brace)?;
        let declaration = &source[start..end];
        match word.as_str() {
            "workflow" => {
                blocks.workflows.push_str(&header);
                blocks.workflows.push_str(declaration);
                blocks.workflows.push('\n');
            }
            "pipeline" => {
                blocks.pipelines.push_str(declaration);
                blocks.pipelines.push('\n');
            }
            "settings" => blocks.settings.push(source[brace + 1..end - 1].to_string()),
            "tests" => blocks.tests.push(source[brace + 1..end - 1].to_string()),
            "package" => blocks.packages.push(source[brace + 1..end - 1].to_string()),
            _ => {
                return Err(RexRapError::syntax(
                    Span::new(start, after_word),
                    format!("unknown top-level RexRap block `{word}`"),
                ));
            }
        }
        cursor = end;
    }
    Ok(blocks)
}

fn skip_space_and_comments(source: &str, mut cursor: usize) -> usize {
    let bytes = source.as_bytes();
    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if source[cursor..].starts_with("//") || source[cursor..].starts_with('#') {
            cursor = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        return cursor;
    }
}

fn read_word(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    if start >= bytes.len() || !(bytes[start].is_ascii_alphabetic() || bytes[start] == b'_') {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some((source[start..end].to_string(), end))
}

fn find_open_brace(source: &str, mut cursor: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                cursor += 2;
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'\"') {
            quote = Some(byte);
        } else if source[cursor..].starts_with("//") || source[cursor..].starts_with('#') {
            cursor = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        } else if byte == b'{' {
            return Some(cursor);
        } else if byte == b'\n' {
            return None;
        }
        cursor += 1;
    }
    None
}

fn matching_brace(source: &str, open: usize) -> Result<usize, RexRapError> {
    let bytes = source.as_bytes();
    let mut cursor = open;
    let mut depth = 0usize;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                cursor += 2;
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'\"') {
            quote = Some(byte);
        } else if source[cursor..].starts_with("//") || source[cursor..].starts_with('#') {
            cursor = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return Ok(cursor + 1);
            }
        }
        cursor += 1;
    }
    Err(RexRapError::syntax(
        Span::new(open, open + 1),
        "unterminated top-level RexRap block",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_multiple_top_level_block_types() {
        let blocks = parse_rrx_blocks(
            r#"language rexrap-1
package { name: "demo" }
settings { secret app.token = "x" }
workflow "A" v1 { let x = console.run() }
pipeline "All" { workflow "A" }
workflow "B" v1 { let y = console.run() }
tests { case "smoke" {} }"#,
        )
        .expect("blocks");
        assert!(blocks.workflows.contains("workflow \"A\""));
        assert!(blocks.workflows.contains("workflow \"B\""));
        assert!(blocks.pipelines.contains("pipeline \"All\""));
        assert_eq!(blocks.settings.len(), 1);
        assert_eq!(blocks.tests.len(), 1);
    }
}
