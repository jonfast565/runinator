#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    pub(super) schema_version: u32,
    pub(super) media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) artifact_type: Option<String>,
    pub(super) config: Descriptor,
    pub(super) layers: Vec<Descriptor>,
}
