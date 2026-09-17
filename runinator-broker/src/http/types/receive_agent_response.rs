#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReceiveAgentResponse {
    pub delivery: AgentDelivery,
}
