#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct ApiParams {
    pub(super) endpoint: String,
    #[serde(default = "default_method")]
    pub(super) method: String,
    #[serde(default)]
    pub(super) body: Option<Value>,
    #[serde(default)]
    pub(super) headers: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) hostname: Option<String>,
    #[serde(default)]
    pub(super) paginate: bool,
}
