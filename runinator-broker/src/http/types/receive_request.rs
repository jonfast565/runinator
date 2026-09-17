#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveRequest {
    pub consumer: String,
    /// when present, the server routes via the targeting-aware `receive_for` path. absent on
    /// pre-targeting clients, which keep the plain general-pool `receive` behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ConsumerProfile>,
}
