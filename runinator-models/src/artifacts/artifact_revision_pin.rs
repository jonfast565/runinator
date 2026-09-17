#[allow(unused_imports)]
use super::*;

/// An exact immutable revision selected by an authored `@revision(N)` pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRevisionPin {
    pub revision: i64,
    /// Canonical content digest for the selected immutable definition.
    pub digest: String,
}
