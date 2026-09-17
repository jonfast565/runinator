#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveWakeResponse {
    pub delivery: WakeDelivery,
}
