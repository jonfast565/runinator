//! turning an inbound http request back into something [`verify_request`] can check.
//!
//! the request is rebuilt from the wire bytes rather than from anything already parsed, because a
//! signature covers exactly what was sent: the raw path, the decoded query pairs, and the header
//! values as written. anywhere this normalises differently from the client, verification fails.

use std::collections::HashMap;

use axum::http::{HeaderMap, Method, Uri};

use runinator_blob_core::sigv4::{
    canonical::{ALGORITHM, UNSIGNED_PAYLOAD},
    verify_request, CanonicalRequest, CredentialStore, PresentedSignature,
};
use runinator_blob_core::{meta::sha256_hex, BlobError};

/// the header carrying the payload hash, and the streaming sentinels a client may put there.
pub const CONTENT_SHA_HEADER: &str = "x-amz-content-sha256";
const STREAMING_PREFIX: &str = "STREAMING-";
const STREAMING_UNSIGNED_TRAILER: &str = "STREAMING-UNSIGNED-PAYLOAD-TRAILER";

/// how the caller described its payload, which decides both what the signature covers and whether
/// the body needs `aws-chunked` decoding before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadDescriptor {
    /// the body's hex sha-256, signed.
    Signed(String),
    /// the body is not covered by the signature.
    Unsigned,
    /// the body is `aws-chunked` framed and not covered by the signature.
    UnsignedChunked,
}

impl PayloadDescriptor {
    /// the value that goes into the canonical request.
    fn canonical_value(&self) -> &str {
        match self {
            PayloadDescriptor::Signed(hash) => hash,
            PayloadDescriptor::Unsigned => UNSIGNED_PAYLOAD,
            PayloadDescriptor::UnsignedChunked => STREAMING_UNSIGNED_TRAILER,
        }
    }

    pub fn is_chunked(&self) -> bool {
        matches!(self, PayloadDescriptor::UnsignedChunked)
    }
}

/// read the payload descriptor a request declared.
pub fn payload_descriptor(headers: &HeaderMap) -> Result<PayloadDescriptor, BlobError> {
    let Some(value) = header(headers, CONTENT_SHA_HEADER) else {
        return Ok(PayloadDescriptor::Unsigned);
    };
    if value == UNSIGNED_PAYLOAD {
        return Ok(PayloadDescriptor::Unsigned);
    }
    if value == STREAMING_UNSIGNED_TRAILER {
        return Ok(PayloadDescriptor::UnsignedChunked);
    }
    if value.starts_with(STREAMING_PREFIX) {
        // per-chunk signature verification is not implemented, and accepting the request without it
        // would mean signing headers while leaving the body unauthenticated under a mode that claims
        // otherwise. refusing says so instead of quietly weakening the guarantee.
        return Err(BlobError::Unauthorized(format!(
            "payload mode '{value}' is not supported; send an unsigned payload \
             (set AWS_REQUEST_CHECKSUM_CALCULATION=when_required for the aws cli) or a hashed body"
        )));
    }
    Ok(PayloadDescriptor::Signed(value))
}

/// verify a request's signature, or accept it unsigned when the store allows that.
pub fn authenticate(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    payload: &PayloadDescriptor,
    store: &CredentialStore,
    region: &str,
) -> Result<(), BlobError> {
    let query = decode_query(uri.query().unwrap_or(""));
    let presented = match presented_signature(headers, &query)? {
        Some(presented) => presented,
        None if store.allows_anonymous() => return Ok(()),
        None => {
            return Err(BlobError::Unauthorized(
                "request is not signed and anonymous access is disabled".into(),
            ))
        }
    };

    // the signed-header list decides which headers enter the canonical request; anything else the
    // client sent is deliberately excluded, exactly as the client excluded it when signing.
    let signed: Vec<String> = presented
        .signed_headers
        .split(';')
        .map(str::to_string)
        .collect();
    let mut canonical_headers = Vec::new();
    for name in &signed {
        let value = header(headers, name).ok_or_else(|| {
            BlobError::Unauthorized(format!(
                "signed header '{name}' is missing from the request"
            ))
        })?;
        canonical_headers.push((name.clone(), value));
    }

    // a presigned url signs every query parameter except the signature itself.
    let signing_query: Vec<(String, String)> = query
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("X-Amz-Signature"))
        .cloned()
        .collect();

    let canonical = CanonicalRequest {
        method: method.as_str(),
        path: uri.path(),
        query: signing_query,
        headers: canonical_headers,
        payload_hash: payload.canonical_value(),
    };
    verify_request(&canonical, &presented, store, region, chrono::Utc::now())
}

/// pull the signature out of the `Authorization` header, or out of the presigned query parameters.
fn presented_signature(
    headers: &HeaderMap,
    query: &[(String, String)],
) -> Result<Option<PresentedSignature>, BlobError> {
    if let Some(authorization) = header(headers, "authorization") {
        return parse_authorization(&authorization, headers).map(Some);
    }
    let lookup: HashMap<&str, &str> = query
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let Some(signature) = lookup.get("X-Amz-Signature") else {
        return Ok(None);
    };
    let algorithm = lookup.get("X-Amz-Algorithm").copied().unwrap_or_default();
    if algorithm != ALGORITHM {
        return Err(BlobError::Unauthorized(format!(
            "unsupported signature algorithm '{algorithm}'"
        )));
    }
    let credential = lookup.get("X-Amz-Credential").ok_or_else(|| {
        BlobError::Unauthorized("presigned url is missing X-Amz-Credential".into())
    })?;
    let (access_key_id, credential_scope) = split_credential(credential)?;
    let expires_in = lookup
        .get("X-Amz-Expires")
        .ok_or_else(|| BlobError::Unauthorized("presigned url is missing X-Amz-Expires".into()))?
        .parse::<i64>()
        .map_err(|_| {
            BlobError::Unauthorized("presigned url has a malformed X-Amz-Expires".into())
        })?;
    Ok(Some(PresentedSignature {
        access_key_id,
        credential_scope,
        signed_headers: lookup
            .get("X-Amz-SignedHeaders")
            .copied()
            .unwrap_or("host")
            .to_string(),
        signature: (*signature).to_string(),
        amz_date: lookup
            .get("X-Amz-Date")
            .copied()
            .ok_or_else(|| BlobError::Unauthorized("presigned url is missing X-Amz-Date".into()))?
            .to_string(),
        expires_in: Some(expires_in),
    }))
}

fn parse_authorization(value: &str, headers: &HeaderMap) -> Result<PresentedSignature, BlobError> {
    let rest = value
        .strip_prefix(ALGORITHM)
        .map(str::trim)
        .ok_or_else(|| {
            BlobError::Unauthorized(
                "authorization header is not an AWS4-HMAC-SHA256 signature".into(),
            )
        })?;
    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;
    for field in rest.split(',') {
        let field = field.trim();
        if let Some(value) = field.strip_prefix("Credential=") {
            credential = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("SignedHeaders=") {
            signed_headers = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("Signature=") {
            signature = Some(value.to_string());
        }
    }
    let credential = credential
        .ok_or_else(|| BlobError::Unauthorized("authorization header has no Credential".into()))?;
    let (access_key_id, credential_scope) = split_credential(&credential)?;
    // sdks send the timestamp in `x-amz-date`, but `date` is the documented fallback.
    let amz_date = header(headers, "x-amz-date")
        .or_else(|| header(headers, "date"))
        .ok_or_else(|| BlobError::Unauthorized("request has no x-amz-date".into()))?;
    Ok(PresentedSignature {
        access_key_id,
        credential_scope,
        signed_headers: signed_headers.ok_or_else(|| {
            BlobError::Unauthorized("authorization header has no SignedHeaders".into())
        })?,
        signature: signature.ok_or_else(|| {
            BlobError::Unauthorized("authorization header has no Signature".into())
        })?,
        amz_date,
        expires_in: None,
    })
}

/// split `AKID/20130524/us-east-1/s3/aws4_request` into the key id and the scope.
fn split_credential(value: &str) -> Result<(String, String), BlobError> {
    value
        .split_once('/')
        .map(|(id, scope)| (id.to_string(), scope.to_string()))
        .ok_or_else(|| BlobError::Unauthorized(format!("malformed credential '{value}'")))
}

/// decode a query string into unencoded `(name, value)` pairs, the form signing operates on.
pub fn decode_query(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(name), percent_decode(value))
        })
        .collect()
}

/// percent-decode, treating `+` literally. s3 does not form-encode its query strings, so decoding
/// `+` as a space would corrupt any key containing one.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// read a header as a string, lowercasing nothing (values are signed as written).
pub fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// verify a body against the hash its sender signed. only meaningful for `Signed` payloads; the
/// unsigned modes have nothing to check against.
pub fn verify_payload(payload: &PayloadDescriptor, body: &[u8]) -> Result<(), BlobError> {
    let PayloadDescriptor::Signed(expected) = payload else {
        return Ok(());
    };
    let actual = sha256_hex(body);
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(BlobError::DigestMismatch {
            expected: expected.clone(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
