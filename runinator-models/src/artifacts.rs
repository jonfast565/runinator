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
}

/// A human-facing, stable-key location. `None` is the package's root namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactPath {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub key: String,
}

impl ArtifactPath {
    pub fn new(namespace: Option<String>, key: impl Into<String>) -> Self {
        Self {
            namespace: namespace.filter(|namespace| !namespace.is_empty()),
            key: key.into(),
        }
    }

    /// Split a dotted authoring path at its final segment. A single segment is rooted.
    pub fn from_qualified(value: impl AsRef<str>) -> Self {
        let value = value.as_ref().trim();
        match value.rsplit_once('.') {
            Some((namespace, key)) if !namespace.is_empty() && !key.is_empty() => {
                Self::new(Some(namespace.to_string()), key)
            }
            _ => Self::new(None, value),
        }
    }

    pub fn qualified(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}.{}", self.key),
            None => self.key.clone(),
        }
    }
}

impl fmt::Display for ArtifactPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.qualified())
    }
}

/// An exact immutable revision selected by an authored `@revision(N)` pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRevisionPin {
    pub revision: i64,
    /// Canonical content digest for the selected immutable definition.
    pub digest: String,
}

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
