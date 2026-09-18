use std::fmt::{Display, Formatter};

use runinator_models::errors::ErrorDescriptor;

pub const CONFIG: ErrorDescriptor =
    ErrorDescriptor::new("JIRA001", "jira.config", "Could not parse URL");
pub const REQUEST_BUILD: ErrorDescriptor =
    ErrorDescriptor::new("JIRA002", "jira.request_build", "Failed to build request");
pub const TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("JIRA003", "jira.timeout", "Request timed out");
pub const CONNECT: ErrorDescriptor =
    ErrorDescriptor::new("JIRA004", "jira.connect", "Could not connect to Jira");
pub const REQUEST: ErrorDescriptor =
    ErrorDescriptor::new("JIRA005", "jira.request", "Request failed");
pub const HTTP_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA006",
    "jira.http_error",
    "Jira returned an error status",
);
pub const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("JIRA007", "jira.invalid_params", "Invalid parameters");
pub const UNSUPPORTED_ACTION: ErrorDescriptor =
    ErrorDescriptor::new("JIRA008", "jira.unsupported_action", "Unsupported action");
pub const IO_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA009",
    "jira.io",
    "Failed to write a downloaded attachment",
);
pub const MISSING_OPERATION_KEY: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA010",
    "jira.missing_operation_key",
    "Reconcilable action needs an operation key",
);
pub const POLL_INCOMPLETE: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA011",
    "jira.poll_incomplete",
    "Jira polling did not reach its end",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    CONFIG,
    REQUEST_BUILD,
    TIMEOUT,
    CONNECT,
    REQUEST,
    HTTP_ERROR,
    INVALID_PARAMS,
    UNSUPPORTED_ACTION,
    IO_ERROR,
    MISSING_OPERATION_KEY,
    POLL_INCOMPLETE,
];

#[derive(Debug)]
pub struct JiraError {
    descriptor: ErrorDescriptor,
    detail: String,
    retry_after_seconds: Option<u64>,
}

impl JiraError {
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
    pub fn config(error: impl Display) -> Self {
        Self::new(CONFIG, error.to_string(), None)
    }
    pub fn json(error: impl Display) -> Self {
        Self::new(HTTP_ERROR, format!("invalid JSON response: {error}"), None)
    }
    pub fn runtime(error: impl Display) -> Self {
        Self::new(REQUEST, error.to_string(), None)
    }
    pub fn request(error: reqwest::Error) -> Self {
        let descriptor = if error.is_builder() {
            REQUEST_BUILD
        } else if error.is_timeout() {
            TIMEOUT
        } else if error.is_connect() {
            CONNECT
        } else {
            REQUEST
        };
        Self::new(descriptor, error_chain(&error), None)
    }
    pub fn status(status: u16, body: String, retry_after_seconds: Option<u64>) -> Self {
        Self::new(
            HTTP_ERROR,
            format!("HTTP {status}: {body}"),
            retry_after_seconds,
        )
    }
    pub fn poll(error: impl Display) -> Self {
        Self::new(POLL_INCOMPLETE, error.to_string(), None)
    }
    fn new(descriptor: ErrorDescriptor, detail: String, retry_after_seconds: Option<u64>) -> Self {
        Self {
            descriptor,
            detail,
            retry_after_seconds,
        }
    }
}

impl Display for JiraError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} - {}: {}",
            self.descriptor.code, self.descriptor.summary, self.detail
        )
    }
}
impl std::error::Error for JiraError {}

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
