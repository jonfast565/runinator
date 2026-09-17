#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveEventResponse {
    pub delivery: EventDelivery,
}
