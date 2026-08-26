use serde::{Deserialize, Serialize};

/// what `POST /artifacts/content` returns: where the bytes landed and what they hashed to.
///
/// The caller records the `uri` on the artifact it is already reporting.
/// The `sha256` value verifies the upload without reading the object again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactContentResponse {
    pub uri: String,
    pub size_bytes: i64,
    pub sha256: String,
}
