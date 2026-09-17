#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) labels: BTreeMap<String, String>,
}
