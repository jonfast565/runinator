//! Stable identities and authored paths for durable Runinator artifacts.
//!
//! A path is a user-facing lookup handle. An [`ArtifactRef`] is the durable edge stored in another
//! artifact: it always carries the target UUID and may retain the authored path for diagnostics and
//! decompilation. Renaming or moving an artifact therefore changes a lookup mapping, not every
//! dependent definition.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The native durable artifact collections. This is deliberately not a polymorphic database
/// table; each kind remains owned by its existing table and lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Workflow,
    Pipeline,
    FunctionPackage,
    Setting,
    ExecutionProfile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_rooted_and_namespaced_paths() {
        let rooted = ArtifactPath::from_qualified("reconcile");
        assert_eq!(rooted.namespace, None);
        assert_eq!(rooted.key, "reconcile");

        let namespaced = ArtifactPath::from_qualified("acme.billing.reconcile");
        assert_eq!(namespaced.namespace.as_deref(), Some("acme.billing"));
        assert_eq!(namespaced.key, "reconcile");
        assert_eq!(namespaced.qualified(), "acme.billing.reconcile");
    }
}

mod artifact_path;
pub use artifact_path::ArtifactPath;

mod artifact_revision_pin;
pub use artifact_revision_pin::ArtifactRevisionPin;

mod artifact_ref;
pub use artifact_ref::ArtifactRef;
