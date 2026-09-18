use std::fmt::{Display, Formatter};

use runinator_models::errors::ErrorDescriptor;

pub const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("GITHUB001", "github.invalid_params", "Invalid parameters");
pub const INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB002",
    "github.invalid_json",
    "Response was not valid JSON",
);
pub const HTTP_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB003",
    "github.http_error",
    "GitHub returned an error status",
);
pub const UNSUPPORTED_ACTION: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB004",
    "github.unsupported_action",
    "Unsupported action",
);
pub const MISSING_REVIEWERS: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB005",
    "github.missing_reviewers",
    "request_reviewers needs at least one reviewer or team_reviewer",
);
pub const MISSING_OPERATION_KEY: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB006",
    "github.missing_operation_key",
    "Reconcilable action needs an operation key",
);
pub const REVISION_MISMATCH: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB007",
    "github.revision_mismatch",
    "GitHub returned a check run for a different revision",
);
pub const MISSING_AUTHENTICATION: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB008",
    "github.missing_authentication",
    "A token or GitHub execution profile is required",
);
pub const CONFLICTING_AUTHENTICATION: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB009",
    "github.conflicting_authentication",
    "Token and execution-profile authentication cannot be combined",
);
pub const POLL_INCOMPLETE: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB010",
    "github.poll_incomplete",
    "GitHub polling did not reach its checkpoint",
);
pub const COMMAND_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB011",
    "github.command_failed",
    "GitHub CLI request failed",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    INVALID_JSON,
    HTTP_ERROR,
    UNSUPPORTED_ACTION,
    MISSING_REVIEWERS,
    MISSING_OPERATION_KEY,
    REVISION_MISMATCH,
    MISSING_AUTHENTICATION,
    CONFLICTING_AUTHENTICATION,
    POLL_INCOMPLETE,
    COMMAND_FAILED,
];

#[derive(Debug)]
pub struct GitHubError {
    descriptor: ErrorDescriptor,
    detail: String,
    retry_after_seconds: Option<u64>,
}

impl GitHubError {
    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn retry_after_seconds(&self) -> Option<u64> {
        self.retry_after_seconds
    }
    pub fn is_rate_limited(&self) -> bool {
        self.retry_after_seconds.is_some() || self.detail.contains("HTTP 429")
    }
    pub fn descriptor(&self) -> ErrorDescriptor {
        self.descriptor
    }
    pub fn request(error: reqwest::Error) -> Self {
        Self::new(HTTP_ERROR, error_chain(&error), None)
    }
    pub fn config(error: impl Display) -> Self {
        Self::new(INVALID_PARAMS, error.to_string(), None)
    }
    pub fn json(error: impl Display) -> Self {
        Self::new(INVALID_JSON, error.to_string(), None)
    }
    pub fn runtime(error: impl Display) -> Self {
        Self::new(HTTP_ERROR, error.to_string(), None)
    }
    pub fn command(error: impl Display) -> Self {
        Self::new(COMMAND_FAILED, error.to_string(), None)
    }
    pub fn timeout() -> Self {
        Self::new(COMMAND_FAILED, "request timed out".into(), None)
    }
    pub fn output_too_large(max: usize) -> Self {
        Self::new(
            COMMAND_FAILED,
            format!("response exceeds {max} bytes"),
            None,
        )
    }
    pub fn status(status: u16, body: String, retry_after_seconds: Option<u64>) -> Self {
        Self::new(
            HTTP_ERROR,
            format!("HTTP {status}: {body}"),
            retry_after_seconds,
        )
    }
    pub fn poll(detail: impl Into<String>) -> Self {
        Self::new(POLL_INCOMPLETE, detail.into(), None)
    }
    fn new(descriptor: ErrorDescriptor, detail: String, retry_after_seconds: Option<u64>) -> Self {
        Self {
            descriptor,
            detail,
            retry_after_seconds,
        }
    }
}

impl Display for GitHubError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} - {}: {}",
            self.descriptor.code, self.descriptor.summary, self.detail
        )
    }
}
impl std::error::Error for GitHubError {}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}
