use pest::iterators::Pair;

use crate::{
    ast::{ExprKind, StrPart},
    errors::{Span, WdlError},
};

use super::{Rule, first_inner, parse_expr, span_of};

pub(super) fn parse_number(text: &str, span: Span) -> Result<ExprKind, WdlError> {
    if text.contains('.') {
        text.parse::<f64>()
            .map(ExprKind::Float)
            .map_err(|_| WdlError::syntax(span, format!("invalid number '{text}'")))
    } else {
        text.parse::<i64>()
            .map(ExprKind::Int)
            .map_err(|_| WdlError::syntax(span, format!("invalid integer '{text}'")))
    }
}

pub(super) fn parse_duration(text: &str, span: Span) -> Result<i64, WdlError> {
    let (digits, unit) = text.split_at(text.len() - 1);
    let amount = digits
        .parse::<i64>()
        .map_err(|_| WdlError::syntax(span, format!("invalid duration '{text}'")))?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        other => {
            return Err(WdlError::syntax(
                span,
                format!("unknown duration unit '{other}'"),
            ));
        }
    };
    Ok(amount * multiplier)
}

pub(super) fn parse_i64(text: &str, span: Span) -> Result<i64, WdlError> {
    text.parse::<i64>()
        .map_err(|_| WdlError::syntax(span, format!("invalid integer '{text}'")))
}

pub(super) fn parse_optional_count(pair: Pair<Rule>) -> Result<Option<i64>, WdlError> {
    match pair
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::integer)
    {
        Some(int) => Ok(Some(parse_i64(int.as_str(), span_of(&int))?)),
        None => Ok(None),
    }
}

pub(super) fn string_parts(pair: Pair<Rule>) -> Result<Vec<StrPart>, WdlError> {
    let mut parts = Vec::new();
    for inner in pair.into_inner() {
        if inner.as_rule() != Rule::str_part {
            continue;
        }
        let token = first_inner(inner)?;
        match token.as_rule() {
            Rule::str_text => push_lit(&mut parts, token.as_str()),
            Rule::escape => push_lit(&mut parts, &decode_escape(token.as_str())),
            Rule::interpolation => parts.push(StrPart::Expr(parse_expr(first_inner(token)?)?)),
            _ => {}
        }
    }
    if parts.is_empty() {
        parts.push(StrPart::Lit(String::new()));
    }
    Ok(parts)
}

fn push_lit(parts: &mut Vec<StrPart>, text: &str) {
    if let Some(StrPart::Lit(last)) = parts.last_mut() {
        last.push_str(text);
    } else {
        parts.push(StrPart::Lit(text.to_string()));
    }
}

fn decode_escape(text: &str) -> String {
    let mut chars = text.chars();
    chars.next();
    match chars.next() {
        Some('n') => "\n".to_string(),
        Some('t') => "\t".to_string(),
        Some('r') => "\r".to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

pub(super) fn raw_block_content(text: &str) -> String {
    let Some(content) = text
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
    else {
        return text.to_string();
    };
    if let Some(stripped) = content.strip_prefix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = content.strip_prefix('\n') {
        stripped.to_string()
    } else {
        content.to_string()
    }
}

pub(super) fn plain_string(pair: Pair<Rule>) -> Result<String, WdlError> {
    let mut out = String::new();
    for part in string_parts(pair)? {
        match part {
            StrPart::Lit(text) => out.push_str(&text),
            StrPart::Expr(_) => {
                return Err(WdlError::lower(
                    "interpolation is not allowed in this position",
                ));
            }
        }
    }
    Ok(out)
}
