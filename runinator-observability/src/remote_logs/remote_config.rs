#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct RemoteConfig {
    pub(super) base: String,
    pub(super) token: Option<String>,
}
