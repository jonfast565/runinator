#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SignalDeliveryRequest {
    pub name: String,
    #[serde(default)]
    pub payload: Value,
}

impl Validate for SignalDeliveryRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("name", &self.name)
    }
}
