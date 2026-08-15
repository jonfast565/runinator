//! a published code package: the unit that owns versions and aliases.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// a named package of code, unique per organization and namespace.
///
/// the package itself carries no code — versions do. it exists so aliases and grants have something
/// stable to point at while versions come and go.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionPackage {
    pub id: Uuid,
    /// the organization that owns it. `None` is platform-global, matching workflows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    /// the namespace qualifying the name, if any. `(org, namespace, name)` is the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// the version the default alias currently resolves to, when one is set. denormalised for
    /// listing: a package list that had to join versions and aliases per row would be a query per
    /// package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FunctionPackage {
    /// the fully qualified name, `namespace.name` or just `name`.
    pub fn qualified_name(&self) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}.{}", self.name),
            None => self.name.clone(),
        }
    }
}
