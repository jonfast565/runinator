#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeViolation {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl TypeViolation {
    pub(super) fn new(
        path: &[String],
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            path: format_path(path),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    pub fn at(path: &[String], expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::new(path, expected, actual)
    }

    pub fn message_with_label(&self, label: &str) -> String {
        let label = Self::label_with_path(label, &self.path);
        if self.actual == "missing" {
            return format!("{label} is missing required field");
        }
        if self.actual == "unexpected" {
            return format!("{label} is not allowed");
        }
        format!("{label} expected {}, got {}", self.expected, self.actual)
    }

    pub fn label_with_path(label: &str, path: &str) -> String {
        let path = path.trim_start_matches('$');
        if path.is_empty() {
            return label.to_string();
        }
        if let Some(prefix) = label.strip_suffix('\'') {
            return format!("{prefix}{path}'");
        }
        format!("{label}{path}")
    }
}

impl std::fmt::Display for TypeViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.actual == "missing" {
            return write!(formatter, "{} is missing required field", self.path);
        }
        if self.actual == "unexpected" {
            return write!(formatter, "{} is not allowed", self.path);
        }
        write!(
            formatter,
            "{} expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

impl std::error::Error for TypeViolation {}
