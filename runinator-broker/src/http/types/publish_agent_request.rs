#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishAgentRequest {
    pub command: AgentCommand,
}
