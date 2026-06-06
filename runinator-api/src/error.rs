use std::error::Error as StdError;

use reqwest::{StatusCode, Url};
use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use thiserror::Error;
use url::ParseError;

/// Result alias for operations within the Runinator API client crate.
pub type Result<T> = std::result::Result<T, ApiError>;

/// Common error representation for API client operations.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resolving the base URL for the web service failed.
    #[error("API001 - failed to resolve Runinator API base URL: {0}")]
    Discovery(#[source] Box<dyn StdError + Send + Sync>),

    /// The provided base URL is malformed.
    #[error("API002 - invalid Runinator API base URL '{url}': {source}")]
    InvalidBaseUrl {
        url: String,
        #[source]
        source: ParseError,
    },

    /// Joining a path onto the resolved base URL failed.
    #[error("API003 - failed to join path '{path}' to base URL '{base}': {source}")]
    InvalidPath {
        base: Url,
        path: String,
        #[source]
        source: ParseError,
    },

    /// The underlying HTTP client returned an error.
    #[error("API004 - Runinator API request error: {0}")]
    Request(#[from] reqwest::Error),

    /// The web service returned a non-success HTTP status.
    #[error("API005 - Runinator API returned {status} for {url}: {message}")]
    Http {
        status: StatusCode,
        url: Url,
        message: String,
    },

    /// The web service returned a JSON shape this client could not parse.
    #[error("API006 - unexpected Runinator API response: {0}")]
    UnexpectedResponse(String),

    /// Building the compiled pack zip before upload failed.
    #[error("API007 - failed to build pack archive: {0}")]
    Pack(String),
}

// numbered error dictionary for the API client crate.
pub const DISCOVERY: ErrorDescriptor =
    ErrorDescriptor::new("API001", "api.discovery", "Failed to resolve API base URL");
pub const INVALID_BASE_URL: ErrorDescriptor =
    ErrorDescriptor::new("API002", "api.invalid_base_url", "Invalid API base URL");
pub const INVALID_PATH: ErrorDescriptor = ErrorDescriptor::new(
    "API003",
    "api.invalid_path",
    "Failed to join path to base URL",
);
pub const REQUEST: ErrorDescriptor =
    ErrorDescriptor::new("API004", "api.request", "API request error");
pub const HTTP: ErrorDescriptor =
    ErrorDescriptor::new("API005", "api.http", "API returned a non-success status");
pub const UNEXPECTED_RESPONSE: ErrorDescriptor = ErrorDescriptor::new(
    "API006",
    "api.unexpected_response",
    "Unexpected API response",
);
pub const PACK: ErrorDescriptor =
    ErrorDescriptor::new("API007", "api.pack", "Failed to build pack archive");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    DISCOVERY,
    INVALID_BASE_URL,
    INVALID_PATH,
    REQUEST,
    HTTP,
    UNEXPECTED_RESPONSE,
    PACK,
];

impl EngineErrors for ApiError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}

impl ApiError {
    pub(crate) fn discovery<E>(error: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        ApiError::Discovery(Box::new(error))
    }
}
