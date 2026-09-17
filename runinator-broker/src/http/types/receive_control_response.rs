#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveControlResponse {
    pub delivery: ControlDelivery,
}
