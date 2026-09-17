#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SettingsBundle {
    #[serde(default = "default_settings_bundle_version")]
    pub version: u32,
    #[serde(default, alias = "secrets")]
    pub settings: Vec<SettingBundleEntry>,
    #[serde(default)]
    pub execution_profiles: Vec<ExecutionProfileBundleEntry>,
}

impl Default for SettingsBundle {
    fn default() -> Self {
        Self {
            version: default_settings_bundle_version(),
            settings: Vec::new(),
            execution_profiles: Vec::new(),
        }
    }
}
