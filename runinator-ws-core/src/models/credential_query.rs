#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct CredentialQuery {
    pub scope: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub kind: SettingKind,
}
