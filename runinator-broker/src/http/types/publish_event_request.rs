#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishEventRequest {
    pub message: EventMessage,
}
