//! deriving the broker relay endpoint from the service URL, so an operator configures one URL
//! rather than a service URL and a separate broker endpoint.

use runinator_models::errors::SendableError;

/// the web service's broker relay endpoint, relative to the service base URL.
pub const RELAY_PATH: &str = runinator_broker::DEFAULT_BROKER_RELAY_PATH;

/// Derive the WS broker relay URL from the service URL.
/// Change `http` to `ws` and `https` to `wss`, then resolve [`RELAY_PATH`].
///
/// Use the same `Url::join` behavior as the API client.
/// A service at `https://host/runinator/` therefore keeps that path prefix.
pub fn derive_relay_url(service_url: &str) -> Result<String, SendableError> {
    derive_relay_url_with_path(service_url, RELAY_PATH)
}

pub fn derive_relay_url_with_path(
    service_url: &str,
    relay_path: &str,
) -> Result<String, SendableError> {
    runinator_broker::derive_websocket_relay_url(service_url, relay_path)
        .map_err(|err| crate::errors::RELAY_URL.error(err.to_string()))
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
