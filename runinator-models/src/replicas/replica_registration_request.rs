#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaRegistrationRequest {
    /// Caller-selected identity for broker-announced replicas. HTTP callers leave this unset and
    /// the registry assigns one; a broker consumer must know its identity before the asynchronous
    /// registration is applied so it can safely stamp executor claims and receive directives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_id: Option<Uuid>,
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub attributes: Value,
}

impl Validate for ReplicaRegistrationRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("instance_id", &self.instance_id)?;
        identifier("runtime_id", &self.runtime_id)?;
        optional_text("display_name", self.display_name.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("host", self.host.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("base_path", self.base_path.as_deref(), 2 * 1024)?;
        optional_text("version", self.version.as_deref(), SHORT_TEXT_MAX)
    }
}
