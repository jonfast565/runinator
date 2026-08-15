//! the one movable pointer at a package version.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// a named pointer at a version, e.g. `production -> 3`.
///
/// aliases are the only mutable part of a published package. moving one changes what *new* calls
/// resolve to and nothing else: a compiled workflow recorded a `FunctionBinding` naming an exact
/// version, so promotion never reaches back into work that already exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionAlias {
    pub id: Uuid,
    pub package_id: Uuid,
    pub name: String,
    pub version_id: Uuid,
    /// the version number `version_id` names, denormalised so a listing can show `production -> 3`
    /// without joining.
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
