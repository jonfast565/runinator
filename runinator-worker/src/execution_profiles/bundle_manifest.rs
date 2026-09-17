#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(super) struct BundleManifest {
    pub(super) version: u32,
    pub(super) profile_id: uuid::Uuid,
    pub(super) config_digest: String,
    pub(super) files: Vec<ManifestFile>,
}
