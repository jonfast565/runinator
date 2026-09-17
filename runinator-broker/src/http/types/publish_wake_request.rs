#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishWakeRequest {
    pub message: WakeMessage,
}
