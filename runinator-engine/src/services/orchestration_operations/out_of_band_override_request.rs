#[allow(unused_imports)]
use super::*;

pub struct OutOfBandOverrideRequest {
    pub target_kind: String,
    pub target_id: Uuid,
    pub action: String,
    pub reason: String,
    pub idempotency_key: String,
    pub actor_id: Option<Uuid>,
}
