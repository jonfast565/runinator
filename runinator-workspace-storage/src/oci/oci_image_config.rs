#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct OciImageConfig {
    pub(super) architecture: String,
    pub(super) os: String,
    pub(super) rootfs: RootFs,
    pub(super) config: RuntimeConfig,
}
