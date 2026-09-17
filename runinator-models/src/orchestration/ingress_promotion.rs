#[allow(unused_imports)]
use super::*;

/// Atomic settlement result handed to the engine when the oldest queued event became the next
/// active generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressPromotion {
    pub admission: IngressAdmission,
    pub event: IngressInboxEntry,
    pub claim_token: Uuid,
}
