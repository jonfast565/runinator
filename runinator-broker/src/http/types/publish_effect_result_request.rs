#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishEffectResultRequest {
    pub message: EffectResultMessage,
}
