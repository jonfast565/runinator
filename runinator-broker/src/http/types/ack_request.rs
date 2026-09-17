#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct AckRequest {
    pub consumer: String,
    pub delivery_id: Uuid,
}
