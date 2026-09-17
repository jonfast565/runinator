#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(super) struct ManifestFile {
    pub(super) path: String,
    pub(super) sha256: String,
    pub(super) size: usize,
}
