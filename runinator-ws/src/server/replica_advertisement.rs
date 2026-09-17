#[allow(unused_imports)]
use super::*;

/// what this web service replica advertises to the replica list at registration and on every
/// heartbeat. host is its stable dns name; attributes carry the broker/database backend it runs on.
#[derive(Debug, Clone, Default)]
pub struct ReplicaAdvertisement {
    pub instance_id: Option<String>,
    pub host: Option<String>,
    pub attributes: runinator_models::value::Value,
}
