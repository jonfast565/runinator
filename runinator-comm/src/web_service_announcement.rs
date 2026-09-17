#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebServiceAnnouncement {
    pub service_id: Uuid,
    pub address: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default = "default_service_scheme")]
    pub scheme: String,
    #[serde(default = "default_relay_path")]
    pub relay_path: String,
    #[serde(default)]
    pub cluster_id: Uuid,
    #[serde(default)]
    pub enrollment_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spki_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
}
