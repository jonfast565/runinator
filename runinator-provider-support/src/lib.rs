//! shared helpers for runinator provider crates.

pub mod process;
pub mod process_runner;
pub mod terminal;

pub use runinator_models::errors::SendableError;
pub use runinator_models::runs::ProviderExecutionRequest;
pub use serde::de::DeserializeOwned;

use runinator_models::errors::ErrorDescriptor;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Apply worker-materialized environment and argument credentials at the provider's chosen point
/// in command construction. Values are deliberately never formatted into diagnostics.
pub fn apply_command_credentials(command: &mut Command, request: &ProviderExecutionRequest) {
    command.envs(&request.credential_injections.environment);
    command.args(&request.credential_injections.arguments);
}

/// Apply worker-materialized credential headers to a blocking HTTP request.
pub fn apply_blocking_http_credentials(
    mut builder: reqwest::blocking::RequestBuilder,
    request: &ProviderExecutionRequest,
) -> Result<reqwest::blocking::RequestBuilder, SendableError> {
    for (name, value) in &request.credential_injections.headers {
        builder = builder.header(name, value);
    }
    Ok(builder)
}

/// Resolve a subprocess working directory while enforcing the worker-provided workspace fence.
/// Relative explicit paths are resolved below the workspace; absolute paths must already be below
/// it. Without an affinity, providers retain their historical explicit-directory behavior.
pub fn resolve_working_dir(
    workspace_path: Option<&str>,
    explicit: Option<&str>,
) -> Result<Option<PathBuf>, SendableError> {
    let explicit = explicit.filter(|value| !value.trim().is_empty());
    let Some(workspace) = workspace_path else {
        return Ok(explicit.map(|value| PathBuf::from(value.trim())));
    };
    let root = std::fs::canonicalize(workspace)?;
    let candidate = match explicit {
        Some(value) if Path::new(value.trim()).is_absolute() => PathBuf::from(value.trim()),
        Some(value) => root.join(value.trim()),
        None => root.clone(),
    };
    let candidate = std::fs::canonicalize(candidate)?;
    if !candidate.starts_with(&root) {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "working directory escapes the assigned workspace",
        )));
    }
    Ok(Some(candidate))
}

/// sanitize a provider-generated filename stem for portable filesystem exports.
pub fn sanitize_file_stem(input: &str) -> String {
    let mut sanitized = input
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if character.is_control() => '_',
            _ => character,
        })
        .collect::<String>();

    sanitized = sanitized
        .trim()
        .trim_matches('.')
        .trim_matches('\'')
        .to_string();

    if sanitized.len() > 120 {
        sanitized.truncate(120);
    }
    sanitized
}

/// deserialize a provider request's parameters into `T`, tagging failures with the
/// caller's invalid-params error descriptor so each provider keeps its own error code.
pub fn parse_params<T: DeserializeOwned>(
    request: &ProviderExecutionRequest,
    invalid: &ErrorDescriptor,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone().into()).map_err(|e| invalid.error(e))
}

/// generate a crate-local generic `parse_params(request)` that delegates to
/// [`parse_params`] with the given invalid-params descriptor path. Lets providers keep
/// their existing call sites while sharing the deserialization logic.
#[macro_export]
macro_rules! provider_parse_params {
    ($invalid:path) => {
        pub(crate) fn parse_params<T: $crate::DeserializeOwned>(
            request: &$crate::ProviderExecutionRequest,
        ) -> ::core::result::Result<T, $crate::SendableError> {
            $crate::parse_params(request, &$invalid)
        }
    };
}

#[cfg(test)]
mod lib_tests;
