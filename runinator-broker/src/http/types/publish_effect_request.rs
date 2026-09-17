#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishEffectRequest {
    pub message: EffectMessage,
}
