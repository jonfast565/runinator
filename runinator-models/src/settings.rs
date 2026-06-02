use serde::{Deserialize, Serialize};

/// classifies a stored setting: a redacted, late-resolved `Secret` or a
/// non-sensitive, eagerly-resolved `Config` value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    #[default]
    Secret,
    Config,
}

impl SettingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingKind::Secret => "secret",
            SettingKind::Config => "config",
        }
    }
}

/// a stored setting's identity, without its value. returned by the list endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingSummary {
    pub scope: String,
    pub name: String,
    #[serde(default)]
    pub kind: SettingKind,
}
