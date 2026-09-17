#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlDelivery {
    pub delivery_id: Uuid,
    pub command: ControlCommand,
    #[serde(default = "utc_now")]
    pub enqueued_at: DateTime<Utc>,
}

impl From<ControlCommand> for ControlDelivery {
    fn from(command: ControlCommand) -> Self {
        Self {
            delivery_id: Uuid::new_v4(),
            command,
            enqueued_at: utc_now(),
        }
    }
}
