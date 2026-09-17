#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDelivery {
    pub delivery_id: Uuid,
    pub command: AgentCommand,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<AgentCommand> for AgentDelivery {
    fn from(command: AgentCommand) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            command,
            enqueued_at: utc_now(),
        }
    }
}
