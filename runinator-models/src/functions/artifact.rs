//! the stored bytes of a package, addressed by content.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// a package archive in the object store.
///
/// keyed by digest rather than by id, which is what makes republishing identical bytes free and
/// makes the same package produce the same artifact on any machine. immutable by construction: a
/// different byte gives a different digest and therefore a different artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionArtifact {
    /// `sha256:<hex>` of the archive.
    pub digest: String,
    pub size_bytes: i64,
    /// where the bytes live, a `blob://` URI.
    pub uri: String,
    pub media_type: String,
    pub created_at: DateTime<Utc>,
}

/// the media type a package archive is stored with.
pub const ARTIFACT_MEDIA_TYPE: &str = "application/zip";

/// the digest prefix every artifact digest carries.
pub const DIGEST_PREFIX: &str = runinator_hash::SHA256_PREFIX;

impl FunctionArtifact {
    /// the hex half of the digest, without the algorithm prefix.
    pub fn digest_hex(&self) -> &str {
        runinator_hash::hex_part(&self.digest)
    }
}

/// render a hex sha-256 as a prefixed digest.
pub fn digest_from_hex(hex: &str) -> String {
    runinator_hash::digest_from_hex(hex)
}

/// true when a string is a well-formed artifact digest. checked wherever a digest crosses a trust
/// boundary, since it is used to build an object key.
pub fn is_valid_digest(digest: &str) -> bool {
    runinator_hash::is_valid_digest(digest)
}
