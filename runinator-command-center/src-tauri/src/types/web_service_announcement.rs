#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebServiceAnnouncement {
    pub service_id: String,
    pub address: String,
    pub port: u16,
    pub base_path: String,
    #[serde(default = "default_service_scheme")]
    pub scheme: String,
    #[serde(default)]
    pub relay_path: String,
    #[serde(default)]
    pub cluster_id: Option<Uuid>,
    #[serde(default)]
    pub enrollment_enabled: bool,
    #[serde(default)]
    pub spki_pin: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
}
