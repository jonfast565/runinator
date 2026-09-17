#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaHeartbeatRequest {
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default)]
    pub attributes: Value,
}

impl Validate for ReplicaHeartbeatRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("runtime_id", &self.runtime_id)?;
        optional_text("display_name", self.display_name.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("host", self.host.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("base_path", self.base_path.as_deref(), 2 * 1024)
    }
}
