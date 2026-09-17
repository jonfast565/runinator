#[allow(unused_imports)]
use super::*;

pub struct WebServiceAdvertiserConfig {
    pub service_id: Uuid,
    pub bind_addr: String,
    pub gossip_port: u16,
    pub extra_targets: Vec<String>,
    pub announce_address: String,
    pub announce_base_path: String,
    pub announce_scheme: String,
    pub announce_relay_path: String,
    pub cluster_id: Uuid,
    pub enrollment_enabled: bool,
    pub spki_pin: Option<String>,
    pub version: Option<String>,
    pub interval_seconds: u64,
    pub shutdown: Arc<Notify>,
    pub service_port: u16,
}
