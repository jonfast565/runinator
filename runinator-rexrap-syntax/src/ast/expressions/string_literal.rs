#[allow(unused_imports)]
use super::*;

/// a string's parsed content and its authored delimiter form.
#[derive(Debug, Clone, PartialEq)]
pub struct StringLiteral {
    pub style: StringStyle,
    pub parts: Vec<StrPart>,
}

impl StringLiteral {
    pub fn quoted(parts: Vec<StrPart>) -> Self {
        Self {
            style: StringStyle::Quoted,
            parts,
        }
    }

    pub fn literal(text: impl Into<String>) -> Self {
        Self::quoted(vec![StrPart::Lit(text.into())])
    }
}
