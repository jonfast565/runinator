//! covers signing and verification against aws's own published signature v4 vectors.
//!
//! the two expected signatures below are the ones in the aws "signature calculations" documentation
//! for s3 (`GET /test.txt` with a `Range` header, and its presigned equivalent). they were also
//! reproduced locally against the aws common runtime signer, so a failure here means this
//! implementation drifted, not that the vector is stale.

use super::*;

const ACCESS_KEY_ID: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const EMPTY_PAYLOAD_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn credential() -> BlobCredential {
    BlobCredential {
        access_key_id: ACCESS_KEY_ID.into(),
        secret_access_key: SECRET.into(),
    }
}

fn signed_at() -> DateTime<Utc> {
    parse_amz_date("20130524T000000Z").unwrap()
}

fn header_signed_request() -> CanonicalRequest<'static> {
    CanonicalRequest {
        method: "GET",
        path: "/test.txt",
        query: Vec::new(),
        headers: vec![
            ("host".into(), "examplebucket.s3.amazonaws.com".into()),
            ("range".into(), "bytes=0-9".into()),
            ("x-amz-content-sha256".into(), EMPTY_PAYLOAD_SHA.into()),
            ("x-amz-date".into(), "20130524T000000Z".into()),
        ],
        payload_hash: EMPTY_PAYLOAD_SHA,
    }
}

fn presigned_request() -> CanonicalRequest<'static> {
    CanonicalRequest {
        method: "GET",
        path: "/test.txt",
        query: vec![
            ("X-Amz-Algorithm".into(), ALGORITHM.into()),
            (
                "X-Amz-Credential".into(),
                format!("{ACCESS_KEY_ID}/20130524/us-east-1/s3/aws4_request"),
            ),
            ("X-Amz-Date".into(), "20130524T000000Z".into()),
            ("X-Amz-Expires".into(), "86400".into()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ],
        headers: vec![("host".into(), "examplebucket.s3.amazonaws.com".into())],
        payload_hash: UNSIGNED_PAYLOAD,
    }
}

#[test]
fn matches_the_aws_header_signed_vector() {
    let signature = sign_request(
        &header_signed_request(),
        &credential(),
        DEFAULT_REGION,
        signed_at(),
    );
    assert_eq!(
        signature.signature,
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
    );
    assert_eq!(
        signature.signed_headers,
        "host;range;x-amz-content-sha256;x-amz-date"
    );
    assert_eq!(
        signature.authorization_header(ACCESS_KEY_ID),
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
         SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
         Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
    );
}

#[test]
fn matches_the_aws_presigned_vector() {
    let signature = sign_request(
        &presigned_request(),
        &credential(),
        DEFAULT_REGION,
        signed_at(),
    );
    assert_eq!(
        signature.signature,
        "aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
    );
}

fn store() -> CredentialStore {
    CredentialStore::new([credential()])
}

fn presented(signature: &Signature, expires_in: Option<i64>) -> PresentedSignature {
    PresentedSignature {
        access_key_id: ACCESS_KEY_ID.into(),
        credential_scope: signature.credential_scope.clone(),
        signed_headers: signature.signed_headers.clone(),
        signature: signature.signature.clone(),
        amz_date: signature.amz_date.clone(),
        expires_in,
    }
}

#[test]
fn verifies_a_signature_it_produced() {
    let request = header_signed_request();
    let signature = sign_request(&request, &credential(), DEFAULT_REGION, signed_at());
    // inside the skew window relative to when the request claims to have been signed.
    let now = signed_at() + Duration::minutes(1);
    verify_request(
        &request,
        &presented(&signature, None),
        &store(),
        DEFAULT_REGION,
        now,
    )
    .unwrap();
}

#[test]
fn rejects_a_tampered_request() {
    let signature = sign_request(
        &header_signed_request(),
        &credential(),
        DEFAULT_REGION,
        signed_at(),
    );
    let mut tampered = header_signed_request();
    tampered.headers[1] = ("range".into(), "bytes=0-99".into());
    let error = verify_request(
        &tampered,
        &presented(&signature, None),
        &store(),
        DEFAULT_REGION,
        signed_at(),
    )
    .unwrap_err();
    assert!(matches!(error, BlobError::Unauthorized(message) if message.contains("mismatch")));
}

#[test]
fn rejects_an_unknown_access_key() {
    let request = header_signed_request();
    let signature = sign_request(&request, &credential(), DEFAULT_REGION, signed_at());
    let mut wrong = presented(&signature, None);
    wrong.access_key_id = "AKIAOTHER".into();
    assert!(verify_request(&request, &wrong, &store(), DEFAULT_REGION, signed_at()).is_err());
}

#[test]
fn rejects_a_skewed_clock_and_an_expired_presign() {
    let request = header_signed_request();
    let signature = sign_request(&request, &credential(), DEFAULT_REGION, signed_at());
    let far_future = signed_at() + Duration::minutes(MAX_CLOCK_SKEW_MINUTES + 1);
    assert!(verify_request(
        &request,
        &presented(&signature, None),
        &store(),
        DEFAULT_REGION,
        far_future
    )
    .is_err());

    let presigned = presigned_request();
    let presigned_signature = sign_request(&presigned, &credential(), DEFAULT_REGION, signed_at());
    // a presign is judged against its own lifetime rather than the skew window, so an hour later is
    // fine for a day-long url but a day and a second later is not.
    verify_request(
        &presigned,
        &presented(&presigned_signature, Some(86_400)),
        &store(),
        DEFAULT_REGION,
        signed_at() + Duration::hours(1),
    )
    .unwrap();
    assert!(verify_request(
        &presigned,
        &presented(&presigned_signature, Some(86_400)),
        &store(),
        DEFAULT_REGION,
        signed_at() + Duration::seconds(86_401),
    )
    .is_err());
}

#[test]
fn encodes_reserved_characters_in_paths_and_queries() {
    // a space and a plus must both become percent escapes; a `+` left alone would be read as a space
    // by the other side and change the signature.
    assert_eq!(canonical::uri_encode("a b+c", true), "a%20b%2Bc");
    assert_eq!(canonical::uri_encode("a/b", false), "a/b");
    assert_eq!(canonical::uri_encode("a/b", true), "a%2Fb");
    assert_eq!(canonical::uri_encode("-_.~", true), "-_.~");
}
