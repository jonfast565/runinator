#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSummary {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub scope: String,
    pub name: String,
    #[serde(default)]
    pub kind: SettingKind,
}
