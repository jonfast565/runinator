//! aws signature v4 canonicalization.
//!
//! this is the half that must be byte-identical on both sides, so it is written once and used by the
//! signer and the verifier alike. every rule here is from the aws spec rather than a choice:
//! uri-encoding twice for the path unless the service is s3 (which encodes once), sorting query
//! parameters by encoded name, lowercasing and trimming signed header values, and joining the
//! canonical request with `\n`.

use sha2::{Digest, Sha256};

/// the payload hash a caller sends when the body is not signed. s3 accepts it for https traffic and
/// for presigned urls, and it is what lets the server verify a request without buffering the body.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// the algorithm token in the `Authorization` header and the `X-Amz-Algorithm` query parameter.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// the request shape signing operates on, independent of any http library.
pub struct CanonicalRequest<'a> {
    pub method: &'a str,
    /// the absolute path **exactly as it appears on the wire**, already percent-encoded.
    ///
    /// s3 signs with single uri encoding (`use_double_uri_encode = false`), so the canonical path is
    /// the request-target path verbatim rather than something re-derived from the key. a server
    /// therefore passes its raw request path and a client passes the path it encoded when building
    /// the url — which is what keeps a key containing `!`, `*`, `'`, `(`, or `)` verifiable, since
    /// sdks percent-encode those but they are legal in a key.
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

/// percent-encode per the aws rules: unreserved characters pass through, everything else becomes
/// uppercase hex. `encode_slash` is false only for path segments of non-s3 services; s3 signs its
/// path with slashes intact and no double encoding, which is why callers pass the raw path here.
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
/// path, which s3 signs as `/`.
fn canonical_path(path: &str) -> &str {
    if path.is_empty() {
        return "/";
    }
    path
}

/// percent-encode an object key into a url path, leaving separators intact. the counterpart a client
/// uses when it builds the url whose path it will then sign verbatim.
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
