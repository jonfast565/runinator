//! aws signature v4, signing and verification.
//!
//! both directions live here so they cannot drift: the client signs with [`sign_request`], the
//! server recomputes with [`verify_request`], and both build the same [`canonical::CanonicalRequest`].
//! that is the whole reason this module sits in the contract crate rather than in the server.
//!
//! coverage is the header-signed form plus the presigned-query form. chunked (`STREAMING-*`) payload
//! signing is not implemented; the server rejects it explicitly rather than accepting a request it
//! cannot actually verify.

pub mod canonical;
pub mod credentials;

use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::errors::BlobError;

pub use canonical::{CanonicalRequest, ALGORITHM, UNSIGNED_PAYLOAD};
pub use credentials::{BlobCredential, CredentialStore, DEFAULT_REGION, SERVICE};

type HmacSha256 = Hmac<Sha256>;

/// how far a request's timestamp may be from ours before it is refused. aws uses the same window;
/// it bounds replay without demanding tight clock sync.
pub const MAX_CLOCK_SKEW_MINUTES: i64 = 15;

/// Longest lifetime allowed for a presigned URL, matching S3.
pub const MAX_PRESIGN_SECONDS: i64 = 7 * 24 * 60 * 60;

/// the `X-Amz-Date` format.
pub const AMZ_DATE_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const SCOPE_DATE_FORMAT: &str = "%Y%m%d";

/// a computed signature plus the pieces a caller needs to put it on the wire.
#[derive(Debug, Clone)]
pub struct Signature {
    pub signature: String,
    pub signed_headers: String,
    pub credential_scope: String,
    pub amz_date: String,
}

impl Signature {
    /// the `Authorization` header value for a header-signed request.
    pub fn authorization_header(&self, access_key_id: &str) -> String {
        format!(
            "{ALGORITHM} Credential={access_key_id}/{}, SignedHeaders={}, Signature={}",
            self.credential_scope, self.signed_headers, self.signature
        )
    }
}

/// the credential scope string, `<date>/<region>/<service>/aws4_request`.
pub fn credential_scope(date: DateTime<Utc>, region: &str) -> String {
    format!(
        "{}/{region}/{SERVICE}/aws4_request",
        date.format(SCOPE_DATE_FORMAT)
    )
}

/// sign a canonical request, returning everything needed to put it on the wire.
pub fn sign_request(
    request: &CanonicalRequest<'_>,
    credential: &BlobCredential,
    region: &str,
    signed_at: DateTime<Utc>,
) -> Signature {
    let scope = credential_scope(signed_at, region);
    let amz_date = signed_at.format(AMZ_DATE_FORMAT).to_string();
    let to_sign = canonical::string_to_sign(&amz_date, &scope, &request.hash());
    let key = signing_key(&credential.secret_access_key, signed_at, region);
    Signature {
        signature: hex::encode(hmac(&key, to_sign.as_bytes())),
        signed_headers: request.signed_headers(),
        credential_scope: scope,
        amz_date,
    }
}

/// what a server extracted from an inbound request before verifying it.
pub struct PresentedSignature {
    pub access_key_id: String,
    pub credential_scope: String,
    pub signed_headers: String,
    pub signature: String,
    pub amz_date: String,
    /// Present only for a presigned URL. Limits how long the signature stays valid.
    pub expires_in: Option<i64>,
}

/// verify a presented signature against a canonical request rebuilt from the same wire bytes.
///
/// the checks are ordered cheapest-first, but every one of them is a hard failure: an expired
/// presign, a skewed clock, an unknown key, and a wrong signature are all `Unauthorized`, and the
/// message says which so an operator can tell a misconfigured clock from a wrong secret.
pub fn verify_request(
    request: &CanonicalRequest<'_>,
    presented: &PresentedSignature,
    store: &CredentialStore,
    region: &str,
    now: DateTime<Utc>,
) -> Result<(), BlobError> {
    let signed_at = parse_amz_date(&presented.amz_date)?;

    if let Some(expires_in) = presented.expires_in {
        if !(0..=MAX_PRESIGN_SECONDS).contains(&expires_in) {
            return Err(BlobError::Unauthorized(format!(
                "presigned lifetime of {expires_in}s is outside 0..={MAX_PRESIGN_SECONDS}"
            )));
        }
        if now > signed_at + Duration::seconds(expires_in) {
            return Err(BlobError::Unauthorized("presigned url has expired".into()));
        }
    } else {
        let skew = (now - signed_at).num_minutes().abs();
        if skew > MAX_CLOCK_SKEW_MINUTES {
            return Err(BlobError::Unauthorized(format!(
                "request timestamp is {skew} minutes away from ours; limit is {MAX_CLOCK_SKEW_MINUTES}"
            )));
        }
    }

    let expected_scope = credential_scope(signed_at, region);
    if presented.credential_scope != expected_scope {
        return Err(BlobError::Unauthorized(format!(
            "credential scope '{}' does not match '{expected_scope}'",
            presented.credential_scope
        )));
    }
    if presented.signed_headers != request.signed_headers() {
        return Err(BlobError::Unauthorized(format!(
            "signed header list '{}' does not match the request",
            presented.signed_headers
        )));
    }

    let secret = store.secret_for(&presented.access_key_id)?;
    let credential = BlobCredential {
        access_key_id: presented.access_key_id.clone(),
        secret_access_key: secret.to_string(),
    };
    let expected = sign_request(request, &credential, region, signed_at);
    if !constant_time_eq(
        expected.signature.as_bytes(),
        presented.signature.as_bytes(),
    ) {
        return Err(BlobError::Unauthorized("signature mismatch".into()));
    }
    Ok(())
}

/// parse an `X-Amz-Date` value.
pub fn parse_amz_date(value: &str) -> Result<DateTime<Utc>, BlobError> {
    chrono::NaiveDateTime::parse_from_str(value, AMZ_DATE_FORMAT)
        .map(|naive| naive.and_utc())
        .map_err(|_| BlobError::Unauthorized(format!("malformed x-amz-date '{value}'")))
}

/// derive the four-step signing key.
fn signing_key(secret: &str, date: DateTime<Utc>, region: &str) -> Vec<u8> {
    let stamp = date.format(SCOPE_DATE_FORMAT).to_string();
    let key = hmac(format!("AWS4{secret}").as_bytes(), stamp.as_bytes());
    let key = hmac(&key, region.as_bytes());
    let key = hmac(&key, SERVICE.as_bytes());
    hmac(&key, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// compare without leaking where two signatures first differ.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |accumulator, (a, b)| accumulator | (a ^ b))
        == 0
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
