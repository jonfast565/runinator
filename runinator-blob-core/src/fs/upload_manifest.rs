#[allow(unused_imports)]
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct UploadManifest {
    pub(super) key: String,
    pub(super) content_type: Option<String>,
    #[serde(default)]
    pub(super) metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) if_none_match: bool,
    #[serde(default)]
    pub(super) expected_sha256: Option<String>,
}
