#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StoredConfig {
    pub(super) value: Value,
    pub(super) schema: Value,
}
