use std::error::Error as StdError;

use reqwest::{StatusCode, Url};
use thiserror::Error;
use url::ParseError;

/// Result alias for operations within the Runinator API client crate.
pub type Result<T> = std::result::Result<T, ApiError>;

/// Common error representation for API client operations.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resolving the base URL for the web service failed.
    #[error("failed to resolve Runinator API base URL: {0}")]
    Discovery(#[source] Box<dyn StdError + Send + Sync>),

    /// The provided base URL is malformed.
    #[error("invalid Runinator API base URL '{url}': {source}")]
    InvalidBaseUrl {
        url: String,
        #[source]
        source: ParseError,
    },

    /// Joining a path onto the resolved base URL failed.
    #[error("failed to join path '{path}' to base URL '{base}': {source}")]
    InvalidPath {
        base: Url,
        path: String,
        #[source]
        source: ParseError,
    },

    /// The underlying HTTP client returned an error.
    #[error("Runinator API request error: {0}")]
    Request(#[from] reqwest::Error),

    /// The web service returned a non-success HTTP status.
    #[error("Runinator API returned {status} for {url}: {message}")]
    Http {
        status: StatusCode,
        url: Url,
        message: String,
    },

    /// The operation requires a task identifier but none was supplied.
    #[error("missing task identifier; cannot complete operation")]
    MissingTaskId,
}

impl ApiError {
    pub(crate) fn discovery<E>(error: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        ApiError::Discovery(Box::new(error))
    }
}
