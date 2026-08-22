//! aws signature v4 canonicalization.
//!
//! The signer and verifier must produce the same bytes, so both use this code.
//! The rules follow the AWS specification: encode paths as required, sort encoded query names,
//! normalize signed headers, and join the canonical request with `\n`.

use sha2::{Digest, Sha256};

/// Payload hash used when the body is not signed. S3 accepts it for HTTPS and presigned URLs.
/// It lets the server verify the request without buffering the body.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// the algorithm token in the `Authorization` header and the `X-Amz-Algorithm` query parameter.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// the request shape signing operates on, independent of any http library.
pub struct CanonicalRequest<'a> {
    pub method: &'a str,
    /// the absolute path **exactly as it appears on the wire**, already percent-encoded.
    ///
    /// S3 uses one URI-encoding pass (`use_double_uri_encode = false`).
    /// Sign the request path exactly as it appears on the wire. This keeps keys containing
    /// `!`, `*`, `'`, `(`, or `)` verifiable.
    pub path: &'a str,
    /// query parameters as `(name, value)`, unencoded and in any order.
    pub query: Vec<(String, String)>,
    /// headers as `(lowercase name, value)`, restricted to the ones being signed.
    pub headers: Vec<(String, String)>,
    /// lowercase hex sha-256 of the body, or [`UNSIGNED_PAYLOAD`].
    pub payload_hash: &'a str,
}

impl CanonicalRequest<'_> {
    /// the signed-header list, semicolon joined, as it appears in the credential scope.
    pub fn signed_headers(&self) -> String {
        let mut names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names.join(";")
    }

    /// render the canonical request string.
    pub fn render(&self) -> String {
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), collapse_whitespace(value)))
            .collect();
        headers.sort_by(|left, right| left.0.cmp(&right.0));
        let canonical_headers: String = headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect();

        let mut query: Vec<(String, String)> = self
            .query
            .iter()
            .map(|(name, value)| (uri_encode(name, true), uri_encode(value, true)))
            .collect();
        query.sort();
        let canonical_query = query
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&");

        format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            self.method,
            canonical_path(self.path),
            canonical_query,
            canonical_headers,
            self.signed_headers(),
            self.payload_hash
        )
    }

    /// lowercase hex sha-256 of the canonical request, which is what the string-to-sign carries.
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.render().as_bytes()))
    }
}

/// the string the signing key is applied to.
pub fn string_to_sign(amz_date: &str, scope: &str, canonical_request_hash: &str) -> String {
    format!("{ALGORITHM}\n{amz_date}\n{scope}\n{canonical_request_hash}")
}

/// Percent-encode a value using AWS rules. Keep unreserved characters and encode the rest as
/// uppercase hex. For S3, leave slashes intact and do not encode the path twice.
pub fn uri_encode(value: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let character = *byte as char;
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '~') {
            out.push(character);
        } else if character == '/' && !encode_slash {
            out.push('/');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// the canonical path. the wire path is already encoded, so the only normalisation is the empty
/// path, which S3 signs as `/`.
fn canonical_path(path: &str) -> &str {
    if path.is_empty() {
        return "/";
    }
    path
}

/// percent-encode an object key into a URL path, leaving separators intact. the counterpart a client
/// uses when it builds the URL whose path it will then sign verbatim.
pub fn encode_path_segments(path: &str) -> String {
    path.split('/')
        .map(|segment| uri_encode(segment, true))
        .collect::<Vec<_>>()
        .join("/")
}

/// trim and collapse internal runs of spaces, as the spec requires for signed header values.
fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// lowercase hex sha-256 of a payload.
pub fn payload_hash(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}
