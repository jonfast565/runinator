//! covers relay URL derivation: scheme swap, path-prefix preservation, and rejected schemes.

use super::*;

#[test]
fn http_becomes_ws_and_https_becomes_wss() {
    assert_eq!(
        derive_relay_url("http://127.0.0.1:8080/").unwrap(),
        "ws://127.0.0.1:8080/ws/desktop-worker"
    );
    assert_eq!(
        derive_relay_url("https://runinator.example.com/").unwrap(),
        "wss://runinator.example.com/ws/desktop-worker"
    );
}

#[test]
fn a_service_url_without_a_trailing_slash_still_resolves() {
    assert_eq!(
        derive_relay_url("https://runinator.example.com").unwrap(),
        "wss://runinator.example.com/ws/desktop-worker"
    );
}

// a remote agent typically reaches the service through an ingress that mounts it under a path
// prefix. joining (rather than replacing the path) keeps the relay under that same prefix, which is
// This matches what the API client does for every other endpoint.
#[test]
fn a_path_prefixed_service_keeps_its_prefix() {
    assert_eq!(
        derive_relay_url("https://example.com/runinator/").unwrap(),
        "wss://example.com/runinator/ws/desktop-worker"
    );
}

#[test]
fn an_unsupported_scheme_is_rejected() {
    let err = derive_relay_url("ftp://example.com/").unwrap_err();
    assert!(err.to_string().contains("RUNI208"), "{err}");
}

#[test]
fn a_malformed_url_is_rejected() {
    assert!(derive_relay_url("not a url").is_err());
}
