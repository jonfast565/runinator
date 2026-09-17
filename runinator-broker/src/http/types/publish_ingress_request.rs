#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct PublishIngressRequest {
    pub message: IngressMessage,
}
