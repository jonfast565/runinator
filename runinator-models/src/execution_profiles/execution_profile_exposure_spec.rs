#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileExposureSpec {
    #[serde(default = "default_spec_version")]
    pub version: u32,
    #[serde(default)]
    pub home_overlay: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl Default for ExecutionProfileExposureSpec {
    fn default() -> Self {
        Self {
            version: default_spec_version(),
            home_overlay: false,
            environment: BTreeMap::new(),
        }
    }
}
