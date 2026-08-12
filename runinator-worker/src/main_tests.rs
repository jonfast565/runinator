use std::ffi::OsString;

use crate::provider_service_url_fallback;

#[test]
fn provider_service_url_uses_api_base_url_when_env_is_missing() {
    assert_eq!(
        provider_service_url_fallback(None, "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}

#[test]
fn provider_service_url_preserves_existing_env() {
    assert_eq!(
        provider_service_url_fallback(
            Some(OsString::from("http://127.0.0.1:9090/")),
            "http://127.0.0.1:8080/",
        ),
        None
    );
}

#[test]
fn provider_service_url_replaces_empty_env() {
    assert_eq!(
        provider_service_url_fallback(Some(OsString::from("  ")), "http://127.0.0.1:8080/"),
        Some(OsString::from("http://127.0.0.1:8080/"))
    );
}
