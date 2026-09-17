#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NamespaceMigrationPlan {
    pub(super) version: u32,
    pub(super) artifacts: Vec<NamespaceMigrationEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) source_diffs: Vec<String>,
}
