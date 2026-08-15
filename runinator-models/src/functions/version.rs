//! one immutable release of a package.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

use super::FunctionRuntimeSpec;

/// a published version, pinned to exactly one artifact.
///
/// immutable once written: a workflow revision or console cell that pinned this version must keep
/// meaning what it meant, and that guarantee is worthless if the row can be edited. republishing
/// identical bytes reuses the artifact but still mints a new version, because "the same code" and
/// "the same release" are different claims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionVersion {
    pub id: Uuid,
    pub package_id: Uuid,
    /// monotonic per package, assigned by the store rather than the publisher.
    pub version: i64,
    /// the sha-256 of the package archive, `sha256:<hex>`.
    pub artifact_digest: String,
    /// the manifest as published, kept verbatim so a later publish can be diffed against it.
    #[serde(default)]
    pub manifest: Value,
    pub runtime: FunctionRuntimeSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}
