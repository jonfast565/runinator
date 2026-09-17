#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArtifactConfig {
    pub(super) format_version: u32,
    pub(super) revision: String,
}
