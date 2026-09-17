#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationInteractionActionRequest {
    #[serde(default)]
    pub input: Value,
}

impl Validate for NotificationInteractionActionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
