#[allow(unused_imports)]
use super::*;

pub(super) struct RexRapValidator;

impl Validator for RexRapValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        let mut stack = Vec::new();
        let mut quote = None;
        let mut escaped = false;
        for character in line.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if let Some(open) = quote {
                if character == open {
                    quote = None;
                }
                continue;
            }
            if matches!(character, '\'' | '"') {
                quote = Some(character);
                continue;
            }
            match character {
                '{' | '[' | '(' => stack.push(character),
                '}' if stack.last() == Some(&'{') => {
                    stack.pop();
                }
                ']' if stack.last() == Some(&'[') => {
                    stack.pop();
                }
                ')' if stack.last() == Some(&'(') => {
                    stack.pop();
                }
                _ => {}
            }
        }
        if quote.is_some() || !stack.is_empty() || line.trim_end().ends_with('\\') {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}
