#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveIngressResponse {
    pub delivery: IngressDelivery,
}
