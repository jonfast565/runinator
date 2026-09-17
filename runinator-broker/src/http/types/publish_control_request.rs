#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishControlRequest {
    pub command: ControlCommand,
}
