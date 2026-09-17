#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImageIndex {
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) media_type: String,
    pub(super) manifests: Vec<Descriptor>,
}
