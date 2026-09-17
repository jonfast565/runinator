#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRequest {
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// RFC 4648 base64 request bytes.
    pub body_base64: String,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub secrets: Value,
}
