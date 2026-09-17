#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NamespaceMigrationEntry {
    pub(super) kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) setting_kind: Option<SettingKind>,
    pub(super) id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) namespace: Option<String>,
    pub(super) key: String,
    pub(super) display_name: String,
    pub(super) current_path: String,
}
