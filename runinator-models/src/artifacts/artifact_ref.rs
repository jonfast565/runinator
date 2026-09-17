#[allow(unused_imports)]
use super::*;

/// A persisted dependency. `id` is authoritative; the optional path preserves what an author
/// wrote without making the edge vulnerable to a later namespace move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub kind: ArtifactKind,
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_pin: Option<ArtifactRevisionPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_path: Option<ArtifactPath>,
}

impl ArtifactRef {
    pub fn current(kind: ArtifactKind, id: Uuid, authored_path: Option<ArtifactPath>) -> Self {
        Self {
            kind,
            id,
            revision_pin: None,
            authored_path,
        }
    }

    pub fn pinned(
        kind: ArtifactKind,
        id: Uuid,
        revision_pin: ArtifactRevisionPin,
        authored_path: Option<ArtifactPath>,
    ) -> Self {
        Self {
            kind,
            id,
            revision_pin: Some(revision_pin),
            authored_path,
        }
    }
}
