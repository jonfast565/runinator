//! enrollment-token encoding and request authentication.

use super::*;

#[test]
fn token_round_trips_service_urls_with_dots_and_paths() {
    let token = EnrollToken::generate(
        "https://runi.example.test/prefix/",
        Some("sha256/abc.def".to_string()),
    );
    assert_eq!(EnrollToken::decode(&token.encode()).unwrap(), token);
}

#[test]
fn proof_covers_the_exact_request() {
    let token = EnrollToken::generate("https://runi.example/", None);
    let proof = token.proof(br#"{"labels":{"zone":"home"}}"#);
    assert!(token.verify_proof(br#"{"labels":{"zone":"home"}}"#, &proof));
    assert!(!token.verify_proof(br#"{"labels":{"zone":"prod"}}"#, &proof));
}

#[test]
fn discovery_binding_authenticates_the_service_metadata() {
    let token = EnrollToken::generate("https://runi.example/", None);
    let encoded = token.encode();
    let replacement = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("https://attacker.example/".as_bytes());
    let mut parts = encoded.split('.').map(str::to_string).collect::<Vec<_>>();
    parts[3] = replacement;
    assert_eq!(
        EnrollToken::decode(&parts.join(".")).unwrap_err(),
        "invalid enrollment token"
    );
}

#[test]
fn malformed_tokens_have_one_opaque_error() {
    for raw in ["", "lbx1.a.b.c", "runi1.a.b.c", "runi1.a.b.c.d.e"] {
        assert_eq!(
            EnrollToken::decode(raw).unwrap_err(),
            "invalid enrollment token"
        );
    }
}
