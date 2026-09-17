#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct RequestParams {
    pub(super) method: String,
    pub(super) url: String,
    #[serde(default)]
    pub(super) headers: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) query: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) body: Option<Value>,
    #[serde(default = "default_body_format")]
    pub(super) body_format: String,
    #[serde(default)]
    pub(super) timeout_seconds: Option<u64>,
    #[serde(default)]
    pub(super) follow_redirects: bool,
    #[serde(default)]
    pub(super) expect_status: Option<Vec<u16>>,
}
