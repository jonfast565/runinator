use runinator_models::errors::{ErrorDescriptor, ProviderErrors, SendableError};

use crate::provider::JiraProvider;

// numbered error dictionary for the jira provider. the dotted `key` stays the
// runtime error code; the message is rendered as "JIRA00N - <summary>: <detail>".
pub(crate) const CONFIG: ErrorDescriptor =
    ErrorDescriptor::new("JIRA001", "jira.config", "Could not parse URL");
pub(crate) const REQUEST_BUILD: ErrorDescriptor =
    ErrorDescriptor::new("JIRA002", "jira.request_build", "Failed to build request");
pub(crate) const TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("JIRA003", "jira.timeout", "Request timed out");
pub(crate) const CONNECT: ErrorDescriptor =
    ErrorDescriptor::new("JIRA004", "jira.connect", "Could not connect to Jira");
pub(crate) const REQUEST: ErrorDescriptor =
    ErrorDescriptor::new("JIRA005", "jira.request", "Request failed");
pub(crate) const HTTP_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA006",
    "jira.http_error",
    "Jira returned an error status",
);
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("JIRA007", "jira.invalid_params", "Invalid parameters");
pub(crate) const UNSUPPORTED_ACTION: ErrorDescriptor =
    ErrorDescriptor::new("JIRA008", "jira.unsupported_action", "Unsupported action");
pub(crate) const IO_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "JIRA009",
    "jira.io",
    "Failed to write a downloaded attachment",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    CONFIG,
    REQUEST_BUILD,
    TIMEOUT,
    CONNECT,
    REQUEST,
    HTTP_ERROR,
    INVALID_PARAMS,
    UNSUPPORTED_ACTION,
    IO_ERROR,
];

impl ProviderErrors for JiraProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}

// reqwest's display for a request error is terse (e.g. just "builder error").
// walk the std::error source chain so the real cause (an invalid url from a bad
// config value, a tls failure, etc.) reaches the worker logs and run output.
pub(crate) fn http_error(context: &str, err: reqwest::Error) -> SendableError {
    let mut detail = err.to_string();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    let descriptor = if err.is_builder() {
        REQUEST_BUILD
    } else if err.is_timeout() {
        TIMEOUT
    } else if err.is_connect() {
        CONNECT
    } else {
        REQUEST
    };
    descriptor.error(format!("{context}: {detail}"))
}

// validates a configured base url and returns an error naming the offending
// value, so a placeholder or empty config setting is obvious from the message.
pub(crate) fn validate_base_url(base_url: &str) -> Result<(), SendableError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(
            CONFIG.error("jira base_url is empty; set config.jira.base_url to your Jira site URL")
        );
    }
    match reqwest::Url::parse(trimmed) {
        Ok(url) if url.scheme() == "http" || url.scheme() == "https" => Ok(()),
        Ok(url) => Err(CONFIG.error(format!(
            "jira base_url \"{base_url}\" has unsupported scheme \"{}\"; expected http or https",
            url.scheme()
        ))),
        Err(e) => Err(CONFIG.error(format!(
            "jira base_url \"{base_url}\" is not a valid URL: {e}"
        ))),
    }
}
