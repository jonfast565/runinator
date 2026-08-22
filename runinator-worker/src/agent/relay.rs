//! deriving the broker relay endpoint from the service URL, so an operator configures one URL
//! rather than a service URL and a separate broker endpoint.

use runinator_models::errors::SendableError;

/// the web service's broker relay endpoint, relative to the service base URL.
pub const RELAY_PATH: &str = "ws/desktop-worker";

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
    let mut url = reqwest::Url::parse(service_url)
        .map_err(|err| crate::errors::RELAY_URL.error(format!("{service_url}: {err}")))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(
                crate::errors::RELAY_URL.error(format!("unsupported service URL scheme '{other}'"))
            );
        }
    };
    url.set_scheme(scheme).map_err(|_| {
        crate::errors::RELAY_URL.error(format!("cannot set scheme on {service_url}"))
    })?;
    url.join(relay_path.trim_start_matches('/'))
        .map(|url| url.to_string())
        .map_err(|err| crate::errors::RELAY_URL.error(format!("{service_url}: {err}")))
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
