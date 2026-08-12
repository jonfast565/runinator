//! deriving the broker relay endpoint from the service url, so an operator configures one url
//! rather than a service url and a separate broker endpoint.

use runinator_models::errors::SendableError;

/// the web service's broker relay endpoint, relative to the service base url.
pub const RELAY_PATH: &str = "ws/desktop-worker";

/// derive the ws broker relay url from the service url: swap the scheme (`http`->`ws`,
/// `https`->`wss`) and resolve [`RELAY_PATH`] against it.
///
/// resolution uses the same `Url::join` the api client uses for every other endpoint, so a service
/// hosted under a path prefix (`https://host/runinator/`) yields a relay url under the same prefix
/// instead of one at the origin root.
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
