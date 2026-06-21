//! shared helpers for runinator provider crates.

pub use runinator_models::errors::SendableError;
pub use runinator_models::runs::ProviderExecutionRequest;
pub use serde::de::DeserializeOwned;

use runinator_models::errors::ErrorDescriptor;

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
