//! covers payload-descriptor reading and query decoding, the two places a signature verification
//! can silently disagree with the client.

use super::*;
use axum::http::HeaderValue;

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(
            name.parse::<axum::http::HeaderName>().unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    headers
}

#[test]
fn reads_the_payload_descriptor() {
    let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(
        payload_descriptor(&headers(&[(CONTENT_SHA_HEADER, hash)])).unwrap(),
        PayloadDescriptor::Signed(hash.into())
    );
    assert_eq!(
        payload_descriptor(&headers(&[(CONTENT_SHA_HEADER, "UNSIGNED-PAYLOAD")])).unwrap(),
        PayloadDescriptor::Unsigned
    );
    assert_eq!(
        payload_descriptor(&headers(&[(
            CONTENT_SHA_HEADER,
            "STREAMING-UNSIGNED-PAYLOAD-TRAILER"
        )]))
        .unwrap(),
        PayloadDescriptor::UnsignedChunked
    );
    // an absent header is treated as unsigned rather than as an error, since a plain `GET` from a
    // presigned url sends none.
    assert_eq!(
        payload_descriptor(&HeaderMap::new()).unwrap(),
        PayloadDescriptor::Unsigned
    );
}

#[test]
fn refuses_chunk_signed_payloads_explicitly() {
    let error = payload_descriptor(&headers(&[(
        CONTENT_SHA_HEADER,
        "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
    )]))
    .unwrap_err();
    // the message must name the remedy; a bare 403 here is very hard to diagnose from a client.
    assert!(
        matches!(error, BlobError::Unauthorized(message) if message.contains("unsigned payload"))
    );
}

#[test]
fn verifies_a_signed_payload_against_the_body() {
    let body = b"contents";
    let hash = sha256_hex(body);
    verify_payload(&PayloadDescriptor::Signed(hash), body).unwrap();
    assert!(matches!(
        verify_payload(&PayloadDescriptor::Signed("0".repeat(64)), body),
        Err(BlobError::DigestMismatch { .. })
    ));
    // the unsigned modes have nothing to check against, so any body passes.
    verify_payload(&PayloadDescriptor::Unsigned, body).unwrap();
}

#[test]
fn decodes_query_pairs_without_form_semantics() {
    let decoded = decode_query("prefix=a%2Fb&plus=a+b&flag&empty=");
    assert_eq!(
        decoded,
        vec![
            ("prefix".to_string(), "a/b".to_string()),
            // `+` is a literal here; treating it as a space would corrupt any key containing one.
            ("plus".to_string(), "a+b".to_string()),
            ("flag".to_string(), String::new()),
            ("empty".to_string(), String::new()),
        ]
    );
    assert!(decode_query("").is_empty());
}
